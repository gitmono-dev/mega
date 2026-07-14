use anyhow::anyhow;
use api_model::common::CommonResult;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use ceres::model::orion_runner::{RunnerStatusResponse, StartRunnerRequest, StartRunnerResponse};
use common::config::BuildConfig;
use orion_scheduler_client::{OrionSchedulerClient, StartRunnerPayload};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::api::{
    MonoApiServiceState, api_common::group_permission::ensure_admin, api_doc::ORION_RUNNER_TAG,
    error::ApiError, oauth::model::LoginUser,
};

pub fn routers() -> OpenApiRouter<MonoApiServiceState> {
    OpenApiRouter::new().nest(
        "/orion/runners",
        OpenApiRouter::new()
            .routes(routes!(start_runner))
            .routes(routes!(get_runner_status)),
    )
}

fn scheduler_client(state: &MonoApiServiceState) -> Result<&OrionSchedulerClient, ApiError> {
    state.orion_scheduler_client().ok_or_else(|| {
        ApiError::with_status(
            StatusCode::SERVICE_UNAVAILABLE,
            anyhow!("Orion scheduler is not configured"),
        )
    })
}

struct RunnerEnv {
    server_ws: String,
    scorpio_base_url: String,
    scorpio_lfs_url: String,
}

fn derive_runner_env(build: &BuildConfig) -> Result<RunnerEnv, ApiError> {
    let raw_domain = build.runner_connect_domain.trim();
    let (base_domain, port) = parse_base_domain(raw_domain)?;
    let (http_scheme, ws_scheme) = http_and_ws_schemes(raw_domain, &base_domain);

    let git_host = subdomain_host("git", &base_domain, port);
    let orion_host = subdomain_host("orion", &base_domain, port);
    let scorpio_base_url = format!("{}://{}", http_scheme, git_host);
    let server_ws = format!("{}://{}/ws", ws_scheme, orion_host);

    Ok(RunnerEnv {
        server_ws,
        scorpio_base_url: scorpio_base_url.clone(),
        scorpio_lfs_url: scorpio_base_url,
    })
}

fn parse_base_domain(raw: &str) -> Result<(String, Option<u16>), ApiError> {
    if raw.is_empty() {
        return Err(ApiError::bad_request(anyhow!(
            "build.runner_connect_domain is not configured"
        )));
    }

    let normalized = if raw.contains("://") {
        raw.to_string()
    } else {
        format!("http://{}", raw.trim().trim_end_matches('/'))
    };
    let url = url::Url::parse(&normalized).map_err(|e| {
        ApiError::bad_request(anyhow!("Invalid build.runner_connect_domain: {}", e))
    })?;

    let mut host = url
        .host_str()
        .ok_or_else(|| ApiError::bad_request(anyhow!("runner_connect_domain has no host")))?
        .to_string();
    if let Some(stripped) = host.strip_prefix("git.") {
        host = stripped.to_string();
    }
    if let Some(stripped) = host.strip_prefix("orion.") {
        host = stripped.to_string();
    }

    Ok((host, url.port()))
}

fn subdomain_host(subdomain: &str, base_domain: &str, port: Option<u16>) -> String {
    match port {
        Some(p) => format!("{}.{base_domain}:{p}", subdomain),
        None => format!("{}.{base_domain}", subdomain),
    }
}

fn http_and_ws_schemes(raw_domain: &str, base_domain: &str) -> (&'static str, &'static str) {
    let raw = raw_domain.trim();
    if raw.starts_with("http://") {
        return ("http", "ws");
    }
    if raw.starts_with("https://") {
        return ("https", "wss");
    }
    if is_local_runner_domain(base_domain) {
        return ("http", "ws");
    }
    ("https", "wss")
}

fn is_local_runner_domain(host: &str) -> bool {
    host == "localhost"
        || host.starts_with("127.0.0.1")
        || host.ends_with(".test")
        || host.ends_with(".local")
}

/// Start a new Orion runner VM via orion-scheduler.
#[utoipa::path(
    post,
    path = "/",
    request_body = StartRunnerRequest,
    responses(
        (status = 200, body = CommonResult<StartRunnerResponse>, content_type = "application/json"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden - admin only"),
        (status = 503, description = "Scheduler not configured"),
        (status = 502, description = "Scheduler unreachable"),
    ),
    tag = ORION_RUNNER_TAG
)]
async fn start_runner(
    user: LoginUser,
    State(state): State<MonoApiServiceState>,
    Json(req): Json<StartRunnerRequest>,
) -> Result<Json<CommonResult<StartRunnerResponse>>, ApiError> {
    ensure_admin(&state, &user).await?;
    let client = scheduler_client(&state)?;
    let build = &state.services().storage().config().build;
    let env = derive_runner_env(build)?;

    let payload = StartRunnerPayload {
        target: req.target,
        replace: req.replace,
        server_ws: env.server_ws,
        scorpio_base_url: env.scorpio_base_url,
        scorpio_lfs_url: env.scorpio_lfs_url,
        image_path: req.image_path,
        image_url: req.image_url,
        image_digest: req.image_digest,
        image_disk_gb: req.image_disk_gb,
        image_cpus: req.image_cpus,
        image_memory_mb: req.image_memory_mb,
    };

    let sched_resp = client.start_runner(payload).await.map_err(|e| {
        ApiError::with_status(
            StatusCode::BAD_GATEWAY,
            anyhow!("Scheduler request failed: {}", e),
        )
    })?;

    if sched_resp.status == "conflict" {
        return Err(ApiError::with_status(
            StatusCode::CONFLICT,
            anyhow!(
                "Runner already provisioning for domain {:?}: {}",
                sched_resp.domain,
                sched_resp.error.unwrap_or_else(|| "conflict".to_string())
            ),
        ));
    }

    let vm_id = sched_resp.vm_id.ok_or_else(|| {
        ApiError::with_status(
            StatusCode::BAD_GATEWAY,
            anyhow!(
                "Scheduler returned no vm_id: {}",
                sched_resp
                    .error
                    .unwrap_or_else(|| sched_resp.status.clone())
            ),
        )
    })?;

    let phase = sched_resp.phase.unwrap_or_else(|| {
        if sched_resp.status == "provisioning" {
            "provisioning".to_string()
        } else if sched_resp.status == "ok" {
            "running".to_string()
        } else {
            sched_resp.status
        }
    });

    Ok(Json(CommonResult::success(Some(StartRunnerResponse {
        vm_id,
        phase,
        domain: sched_resp.domain,
    }))))
}

/// Get provisioning/running status for a runner VM.
#[utoipa::path(
    get,
    path = "/{id}",
    params(
        ("id" = String, Path, description = "VM ID returned by start_runner")
    ),
    responses(
        (status = 200, body = CommonResult<RunnerStatusResponse>, content_type = "application/json"),
        (status = 404, description = "VM not found"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden - admin only"),
        (status = 503, description = "Scheduler not configured"),
        (status = 502, description = "Scheduler unreachable"),
    ),
    tag = ORION_RUNNER_TAG
)]
async fn get_runner_status(
    user: LoginUser,
    State(state): State<MonoApiServiceState>,
    Path(id): Path<String>,
) -> Result<Json<CommonResult<RunnerStatusResponse>>, ApiError> {
    ensure_admin(&state, &user).await?;
    let client = scheduler_client(&state)?;

    let sched = client.get_vm_status(&id).await.map_err(|e| {
        ApiError::with_status(
            StatusCode::BAD_GATEWAY,
            anyhow!("Scheduler request failed: {}", e),
        )
    })?;

    let phase = sched
        .phase
        .clone()
        .or_else(|| Some(sched.status.clone()))
        .unwrap_or_else(|| "unknown".to_string());

    if phase == "no_vm" || sched.vm_id.as_deref() != Some(id.as_str()) {
        return Err(ApiError::not_found(anyhow!("Runner VM '{}' not found", id)));
    }

    Ok(Json(CommonResult::success(Some(RunnerStatusResponse {
        vm_id: id,
        phase,
        domain: sched.domain,
        vm_ip: sched.vm_ip,
        log_file: sched.log_file,
        error: sched.error,
        uptime_secs: sched.uptime_secs,
    }))))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_build(domain: &str, orion: &str) -> BuildConfig {
        BuildConfig {
            enable_build: true,
            orion_server: orion.into(),
            orion_preheat_shallow_depth: 0,
            orion_scheduler_url: String::new(),
            orion_scheduler_token: String::new(),
            runner_connect_domain: domain.into(),
        }
    }

    #[test]
    fn derive_runner_env_from_base_domain() {
        let env =
            derive_runner_env(&sample_build("gitmega.com", "https://orion.gitmega.com")).unwrap();
        assert_eq!(env.scorpio_base_url, "https://git.gitmega.com");
        assert_eq!(env.scorpio_lfs_url, "https://git.gitmega.com");
        assert_eq!(env.server_ws, "wss://orion.gitmega.com/ws");
    }

    #[test]
    fn derive_runner_env_local_http() {
        let env = derive_runner_env(&sample_build(
            "gitmono.test:8080",
            "http://orion.gitmono.test:8004",
        ))
        .unwrap();
        assert_eq!(env.scorpio_base_url, "http://git.gitmono.test:8080");
        assert_eq!(env.server_ws, "ws://orion.gitmono.test:8080/ws");
    }

    #[test]
    fn derive_runner_env_strips_git_prefix() {
        let env = derive_runner_env(&sample_build(
            "git.gitmega.com",
            "https://orion.gitmega.com",
        ))
        .unwrap();
        assert_eq!(env.scorpio_base_url, "https://git.gitmega.com");
        assert_eq!(env.server_ws, "wss://orion.gitmega.com/ws");
    }

    #[test]
    fn derive_runner_env_https_orion_uses_wss() {
        let env =
            derive_runner_env(&sample_build("example.com", "https://orion.example.com")).unwrap();
        assert_eq!(env.scorpio_base_url, "https://git.example.com");
        assert_eq!(env.server_ws, "wss://orion.example.com/ws");
    }

    #[test]
    fn derive_runner_env_uses_tls_for_public_domain_even_if_orion_server_is_local() {
        let env = derive_runner_env(&sample_build("xuanwu.openatom.cn", "http://localhost:8004"))
            .unwrap();
        assert_eq!(env.scorpio_base_url, "https://git.xuanwu.openatom.cn");
        assert_eq!(env.scorpio_lfs_url, "https://git.xuanwu.openatom.cn");
        assert_eq!(env.server_ws, "wss://orion.xuanwu.openatom.cn/ws");
    }

    #[test]
    fn derive_runner_env_rejects_missing_domain() {
        assert!(derive_runner_env(&sample_build("", "http://orion.test:8004")).is_err());
    }
}
