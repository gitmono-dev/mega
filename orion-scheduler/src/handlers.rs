use std::{
    hash::{DefaultHasher, Hash, Hasher},
    sync::Arc,
};

use axum::{
    extract::{
        Path, Query, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::StatusCode,
    response::{
        IntoResponse, Json,
        sse::{Event, Sse},
    },
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::time::interval;

use crate::{
    config::{DefaultImageConfig, TargetConfig},
    orion_deployer,
    state::{AppState, VmPhase},
    vm_cleanup,
};

/// Image parameters that can be passed via webhook API to override config-based image selection.
#[derive(Debug, Clone, Default)]
pub struct ImageParams {
    pub path: Option<String>,
    pub url: Option<String>,
    pub digest: Option<String>,
    pub disk_gb: Option<u32>,
    pub cpus: Option<u32>,
    pub memory_mb: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct WebhookResponse {
    pub status: String,
    pub vm_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    pub error: Option<String>,
    /// Path to the log file (not the contents)
    pub orion_log_file: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GithubWebhookPayload {
    pub action: Option<String>,
    /// Optional label for logs (legacy GHA field).
    #[serde(default)]
    pub target: Option<String>,
    /// When true, block until VM provisioning completes (legacy GHA behavior).
    #[serde(default)]
    pub sync: bool,
    /// Force recreate when a Running VM exists for the same domain.
    #[serde(default)]
    pub replace: bool,
    pub server_ws: String,
    pub scorpio_base_url: String,
    pub scorpio_lfs_url: String,
    /// Override image path (local qcow2 file). Overrides default_image from config.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_path: Option<String>,
    /// Override image URL (remote HTTPS). Overrides default_image from config.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
    /// SHA256/SHA512 digest for the image (required when image_path or image_url is set).
    /// Format: "sha256:..." or "sha512:..."
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_digest: Option<String>,
    /// VM disk size in GB.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_disk_gb: Option<u32>,
    /// Number of vCPUs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_cpus: Option<u32>,
    /// VM memory in MB.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_memory_mb: Option<u32>,
    /// When set, write `ORION_RETAIN_ANTARES_MOUNTS` into the guest `.env`
    /// (`true`→`1`, `false`→`0`). Omitted → leave `.env.prod` value unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retain_antares_mounts: Option<bool>,
}

/// Merge webhook image overrides with scheduler `default_image` config.
pub fn merge_image_params(
    payload: &GithubWebhookPayload,
    default: &DefaultImageConfig,
) -> ImageParams {
    let url = payload.image_url.clone();
    let path = if url.is_some() {
        payload.image_path.clone()
    } else {
        payload
            .image_path
            .clone()
            .or_else(|| Some(default.image_path.clone()))
    };
    let digest = payload.image_digest.clone().or_else(|| {
        if path.is_some() || url.is_some() {
            Some(default.image_digest.clone())
        } else {
            None
        }
    });

    ImageParams {
        path,
        url,
        digest,
        disk_gb: payload.image_disk_gb.or(Some(default.image_disk_gb)),
        cpus: payload.image_cpus.or(Some(default.image_cpus)),
        memory_mb: payload.image_memory_mb.or(Some(default.image_memory_mb)),
    }
}

#[cfg(test)]
mod merge_tests {
    use super::*;
    use crate::config::DefaultImageConfig;

    #[test]
    fn merge_uses_defaults_when_payload_omits_image_fields() {
        let default = DefaultImageConfig::default();
        let payload = GithubWebhookPayload {
            action: None,
            target: None,
            sync: false,
            replace: false,
            server_ws: "ws://orion.test/ws".into(),
            scorpio_base_url: "http://git.test".into(),
            scorpio_lfs_url: "http://git.test".into(),
            image_path: None,
            image_url: None,
            image_digest: None,
            image_disk_gb: None,
            image_cpus: None,
            image_memory_mb: None,
            retain_antares_mounts: None,
        };
        let merged = merge_image_params(&payload, &default);
        assert_eq!(merged.path.as_deref(), Some(default.image_path.as_str()));
        assert_eq!(
            merged.digest.as_deref(),
            Some(default.image_digest.as_str())
        );
        assert_eq!(merged.disk_gb, Some(default.image_disk_gb));
    }

    #[test]
    fn merge_payload_overrides_default_disk() {
        let default = DefaultImageConfig::default();
        let payload = GithubWebhookPayload {
            action: None,
            target: None,
            sync: false,
            replace: false,
            server_ws: "ws://orion.test/ws".into(),
            scorpio_base_url: "http://git.test".into(),
            scorpio_lfs_url: "http://git.test".into(),
            image_path: None,
            image_url: None,
            image_digest: None,
            image_disk_gb: Some(64),
            image_cpus: None,
            image_memory_mb: None,
            retain_antares_mounts: None,
        };
        let merged = merge_image_params(&payload, &default);
        assert_eq!(merged.disk_gb, Some(64));
        assert_eq!(merged.cpus, Some(default.image_cpus));
    }
}

/// GET /webhook
pub async fn webhook_get_handler() -> Json<WebhookResponse> {
    Json(WebhookResponse {
        status: "ok".to_string(),
        vm_id: None,
        domain: None,
        phase: None,
        error: None,
        orion_log_file: None,
    })
}

fn vm_json(vm: &crate::state::VmInfo) -> serde_json::Value {
    let phase = vm.phase.as_str();
    let uptime_secs = if vm.phase == VmPhase::Running {
        Some(vm.created_at.elapsed().as_secs())
    } else {
        None
    };
    serde_json::json!({
        "status": phase,
        "phase": phase,
        "vm_id": vm.id,
        "domain": vm.domain,
        "target": vm.target,
        "vm_ip": vm.ip,
        "uptime_secs": uptime_secs,
        "log_file": vm.log_file,
        "error": vm.error,
        "image_path": vm.image_path,
        "image_digest": vm.image_digest,
        "image_cpus": vm.image_cpus,
        "image_memory_mb": vm.image_memory_mb,
        "image_disk_gb": vm.image_disk_gb,
        "image_name": vm.image_name,
        "image_built_at": vm.image_built_at,
        "toolchain_rust": vm.toolchain_rust,
        "toolchain_buck2": vm.toolchain_buck2,
        "toolchain_python": vm.toolchain_python,
        "kernel": vm.kernel,
    })
}

/// POST /webhook - receives update requests (async by default; one VM per server_ws domain).
pub async fn webhook_post_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<GithubWebhookPayload>,
) -> impl IntoResponse {
    tracing::info!(
        "Received webhook: action={:?}, target={:?}, sync={}, replace={}, server_ws={}",
        payload.action,
        payload.target,
        payload.sync,
        payload.replace,
        payload.server_ws,
    );

    if let Err(e) = orion_deployer::validate_runner_env(
        &payload.server_ws,
        &payload.scorpio_base_url,
        &payload.scorpio_lfs_url,
    ) {
        tracing::error!("Invalid runner env: {:?}", e);
        return (
            StatusCode::BAD_REQUEST,
            Json(WebhookResponse {
                status: "error".to_string(),
                vm_id: None,
                domain: None,
                phase: None,
                error: Some(e.to_string()),
                orion_log_file: None,
            }),
        )
            .into_response();
    }

    let domain = match orion_deployer::domain_from_server_ws(&payload.server_ws) {
        Ok(d) => d,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(WebhookResponse {
                    status: "error".to_string(),
                    vm_id: None,
                    domain: None,
                    phase: None,
                    error: Some(e.to_string()),
                    orion_log_file: None,
                }),
            )
                .into_response();
        }
    };

    // Conflict / idempotency checks (hold update lock briefly).
    {
        let _guard = state.lock_update().await;
        if let Some(existing) = state.get_vm_by_domain(&domain).await {
            match existing.phase {
                VmPhase::Provisioning => {
                    return (
                        StatusCode::CONFLICT,
                        Json(WebhookResponse {
                            status: "conflict".to_string(),
                            vm_id: Some(existing.id.clone()),
                            domain: Some(domain),
                            phase: Some(existing.phase.as_str().to_string()),
                            error: Some("VM already provisioning for this domain".to_string()),
                            orion_log_file: existing.log_file.clone(),
                        }),
                    )
                        .into_response();
                }
                VmPhase::Running if !payload.replace => {
                    return (
                        StatusCode::OK,
                        Json(WebhookResponse {
                            status: "ok".to_string(),
                            vm_id: Some(existing.id.clone()),
                            domain: Some(domain),
                            phase: Some(existing.phase.as_str().to_string()),
                            error: None,
                            orion_log_file: existing.log_file.clone(),
                        }),
                    )
                        .into_response();
                }
                VmPhase::Running | VmPhase::Failed => {
                    // replace=true or Failed: allow recreate (handle_update will shut down).
                }
            }
        } else if let Some(max) = state.config.read().await.max_vms() {
            let count = state.vm_count().await;
            if count >= max {
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(WebhookResponse {
                        status: "error".to_string(),
                        vm_id: None,
                        domain: Some(domain),
                        phase: None,
                        error: Some(format!("max_vms limit reached ({max})")),
                        orion_log_file: None,
                    }),
                )
                    .into_response();
            }
        }
    }

    let cfg = state.config.read().await;
    let default_image = cfg.default_image().clone();
    let config_retain = cfg.retain_antares_mounts();
    drop(cfg);
    let image_params = merge_image_params(&payload, &default_image);

    let target_config = TargetConfig {
        server_ws: payload.server_ws.clone(),
        scorpio_base_url: payload.scorpio_base_url.clone(),
        scorpio_lfs_url: payload.scorpio_lfs_url.clone(),
        retain_antares_mounts: payload.retain_antares_mounts.or(config_retain),
    };

    let vm_id = format!("orion-vm-{}", orion_deployer::chrono_lite_timestamp());
    let label = payload
        .target
        .clone()
        .unwrap_or_else(|| "webhook".to_string());
    let domain_for_task = domain.clone();

    if payload.sync {
        let state_clone = state.clone();
        let vm_id_clone = vm_id.clone();
        let result = tokio::task::spawn_blocking(move || {
            let rt = tokio::runtime::Handle::current();
            rt.block_on(orion_deployer::handle_update(
                &state_clone,
                &domain_for_task,
                &label,
                &vm_id_clone,
                target_config,
                image_params,
            ))
        })
        .await;

        return match result {
            Ok(Ok(_vm_id)) => {
                tracing::info!("Successfully created VM: {}", _vm_id);
                let orion_log_file = state.get_vm_by_id(&_vm_id).await.and_then(|vm| vm.log_file);
                (
                    StatusCode::OK,
                    Json(WebhookResponse {
                        status: "ok".to_string(),
                        vm_id: Some(_vm_id),
                        domain: Some(domain),
                        phase: Some("running".to_string()),
                        error: None,
                        orion_log_file,
                    }),
                )
                    .into_response()
            }
            Ok(Err(e)) => {
                tracing::error!("Failed to handle update: {:?}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(WebhookResponse {
                        status: "error".to_string(),
                        vm_id: Some(vm_id),
                        domain: Some(domain),
                        phase: Some("failed".to_string()),
                        error: Some(e.to_string()),
                        orion_log_file: None,
                    }),
                )
                    .into_response()
            }
            Err(e) => {
                tracing::error!("Task join error: {:?}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(WebhookResponse {
                        status: "error".to_string(),
                        vm_id: None,
                        domain: Some(domain),
                        phase: None,
                        error: Some(e.to_string()),
                        orion_log_file: None,
                    }),
                )
                    .into_response()
            }
        };
    }

    let state_clone = state.clone();
    let vm_id_for_task = vm_id.clone();
    // Async path: return 202 immediately, provision in background.
    tokio::spawn(async move {
        let result = tokio::task::spawn_blocking(move || {
            let rt = tokio::runtime::Handle::current();
            rt.block_on(orion_deployer::handle_update(
                &state_clone,
                &domain_for_task,
                &label,
                &vm_id_for_task,
                target_config,
                image_params,
            ))
        })
        .await;

        match result {
            Ok(Ok(id)) => tracing::info!("Background VM provisioning completed: {}", id),
            Ok(Err(e)) => tracing::error!("Background VM provisioning failed: {:?}", e),
            Err(e) => tracing::error!("Background task join error: {:?}", e),
        }
    });

    (
        StatusCode::ACCEPTED,
        Json(WebhookResponse {
            status: "provisioning".to_string(),
            vm_id: Some(vm_id),
            domain: Some(domain),
            phase: Some("provisioning".to_string()),
            error: None,
            orion_log_file: None,
        }),
    )
        .into_response()
}

/// GET /health
pub async fn health_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "healthy",
        "service": "orion-scheduler"
    }))
}

/// GET /status — list all VMs (optional ?domain= filter).
#[derive(Debug, Deserialize, Default)]
pub struct StatusQuery {
    pub domain: Option<String>,
    pub vm_id: Option<String>,
}

pub async fn status_handler(
    State(state): State<Arc<AppState>>,
    Query(q): Query<StatusQuery>,
) -> Json<serde_json::Value> {
    if let Some(id) = q.vm_id.as_deref() {
        return match orion_deployer::get_status_by_id(&state, id).await {
            Some(vm) => Json(vm_json(&vm)),
            None => Json(serde_json::json!({
                "status": "no_vm",
                "phase": "no_vm",
                "vm_id": id
            })),
        };
    }
    if let Some(domain) = q.domain.as_deref() {
        return match orion_deployer::get_status_by_domain(&state, domain).await {
            Some(vm) => Json(vm_json(&vm)),
            None => Json(serde_json::json!({
                "status": "no_vm",
                "phase": "no_vm",
                "domain": domain,
                "vm_id": null
            })),
        };
    }

    let list = orion_deployer::get_status(&state).await;
    let vms: Vec<_> = list.iter().map(vm_json).collect();
    Json(serde_json::json!({
        "status": "ok",
        "count": vms.len(),
        "vms": vms
    }))
}

/// GET /vms/{id}
pub async fn vm_by_id_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match orion_deployer::get_status_by_id(&state, &id).await {
        Some(vm) => (StatusCode::OK, Json(vm_json(&vm))).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "status": "no_vm",
                "phase": "no_vm",
                "vm_id": id,
                "error": "VM not found"
            })),
        )
            .into_response(),
    }
}

/// Format a single log line with colors based on content type
fn format_log_line(line: &str) -> String {
    // Remove ANSI escape codes for clean formatting
    let clean_line = strip_ansi(line);

    // Determine line type and color
    if clean_line.contains("preflight.sh") || clean_line.contains("预检") {
        format!("  🔍 {}", colorize(&clean_line, "cyan"))
    } else if clean_line.contains("cleanup.sh") || clean_line.contains("清理") {
        format!("  🧹 {}", colorize(&clean_line, "yellow"))
    } else if clean_line.contains("systemd") || clean_line.contains("Started") {
        format!("  ✅ {}", colorize(&clean_line, "green"))
    } else if clean_line.contains("ORION_WORKER_ID") || clean_line.contains("Worker ID") {
        format!("  🆔 {}", colorize(&clean_line, "magenta"))
    } else if clean_line.contains("WebSocket") || clean_line.contains("Connecting") {
        format!("  🌐 {}", colorize(&clean_line, "blue"))
    } else if clean_line.contains("Antares") || clean_line.contains("Dicfuse") {
        format!("  📦 {}", colorize(&clean_line, "bright_blue"))
    } else if clean_line.contains("ERROR") || clean_line.contains("error") {
        format!("  ❌ {}", colorize(&clean_line, "red"))
    } else if clean_line.contains("WARN") || clean_line.contains("warn") {
        format!("  ⚠️  {}", colorize(&clean_line, "yellow"))
    } else if clean_line.contains("INFO") || clean_line.contains("info") {
        format!("  ℹ️  {}", colorize(&clean_line, "white"))
    } else if clean_line.starts_with("==>") {
        format!("  ▶️  {}", colorize(&clean_line, "bright_white"))
    } else if clean_line.contains("DEBUG") {
        format!("  🔧 {}", colorize(&clean_line, "dim"))
    } else if clean_line.is_empty() {
        "  ".to_string()
    } else {
        format!("  │  {}", clean_line)
    }
}

/// Apply ANSI color code to text
/// Colors: red, green, yellow, blue, magenta, cyan, white, bright_white, bright_blue, dim
fn colorize(text: &str, color: &str) -> String {
    let code = match color {
        "red" => "31",
        "green" => "32",
        "yellow" => "33",
        "blue" => "34",
        "magenta" => "35",
        "cyan" => "36",
        "white" => "37",
        "bright_white" => "97",
        "bright_blue" => "94",
        "dim" => "90",
        _ => "37",
    };
    format!("\x1b[{}m{}\x1b[0m", code, text)
}

/// Remove ANSI escape sequences (color codes) from text for clean formatting
fn strip_ansi(text: &str) -> String {
    let mut result = String::new();
    let chars = text.chars().collect::<Vec<_>>();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\x1b' && i + 1 < chars.len() && chars[i + 1] == '[' {
            // Skip until end of ANSI sequence
            i += 2;
            while i < chars.len() && !chars[i].is_ascii_alphabetic() {
                i += 1;
            }
            i += 1; // Skip the final letter
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }
    result
}

/// GET /scorpio/status - Check Scorpio mount status and directories
#[derive(Debug, Deserialize, Default)]
pub struct VmSelectQuery {
    pub domain: Option<String>,
    pub vm_id: Option<String>,
}

pub async fn scorpio_status_handler(
    State(state): State<Arc<AppState>>,
    Query(q): Query<VmSelectQuery>,
) -> impl IntoResponse {
    let key = q.domain.or(q.vm_id);
    match orion_deployer::get_scorpio_status(&state, key.as_deref()).await {
        Ok(status) => (StatusCode::OK, Json(status)).into_response(),
        Err(e) => {
            let response = serde_json::json!({
                "status": "error",
                "error": e.to_string()
            });
            (StatusCode::INTERNAL_SERVER_ERROR, Json(response)).into_response()
        }
    }
}

/// GET /scorpio/config - Read scorpio.toml content from VM
pub async fn scorpio_config_handler(
    State(state): State<Arc<AppState>>,
    Query(q): Query<VmSelectQuery>,
) -> impl IntoResponse {
    let key = q.domain.or(q.vm_id);
    let machine = match orion_deployer::resolve_machine_for_handlers(&state, key.as_deref()).await {
        Ok(m) => m,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "status": "error",
                    "error": e.to_string()
                })),
            )
                .into_response();
        }
    };

    match machine
        .exec("cat /home/orion/orion-runner/scorpio.toml")
        .await
    {
        Ok(output) => {
            let content = String::from_utf8_lossy(&output.stdout).to_string();
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "status": "ok",
                    "path": "/home/orion/orion-runner/scorpio.toml",
                    "content": content
                })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "status": "error",
                "error": e.to_string()
            })),
        )
            .into_response(),
    }
}

/// POST /shutdown — stop one VM; require `?domain=` or `?vm_id=`
/// (use `POST /shutdown/all` to stop every tracked VM). Server keeps running.
#[derive(Debug, Deserialize, Default)]
pub struct ShutdownQuery {
    pub domain: Option<String>,
    pub vm_id: Option<String>,
}

pub async fn shutdown_handler(
    State(state): State<Arc<AppState>>,
    Query(q): Query<ShutdownQuery>,
) -> impl IntoResponse {
    tracing::info!(
        "[http-shutdown] Received shutdown request domain={:?} vm_id={:?}",
        q.domain,
        q.vm_id
    );

    // Serialize with `handle_update`. Without this guard, /shutdown can
    // run between an in-flight create's `KeepAliveMachine::new` and its
    // `state.set_vm`, see an empty domain slot, return success, and leave
    // the freshly-spawned qemu untracked once /webhook publishes it.
    let _update_guard = state.lock_update().await;

    let domain = if let Some(d) = q.domain {
        d
    } else if let Some(id) = q.vm_id {
        match state.domain_for_vm_id(&id).await {
            Some(d) => d,
            None => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({
                        "status": "error",
                        "error": format!("VM '{}' not found", id)
                    })),
                )
                    .into_response();
            }
        }
    } else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "status": "error",
                "error": "Specify ?domain= or ?vm_id= (or POST /shutdown/all)"
            })),
        )
            .into_response();
    };

    if let Err(e) = orion_deployer::shutdown_domain(&state, &domain).await {
        tracing::error!("[http-shutdown] failed: {e}");
    }

    // Disk-side cleanup: qlean only removes the run dir from `Machine::drop`,
    // which doesn't run on SIGKILL/abort. Sweep any orphaned overlay/seed
    // files so /shutdown actually frees the VM's disk footprint, not just
    // its processes. (No host-wide pkill — other domains must stay up.)
    vm_cleanup::sweep_stale_runs().await;

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "ok",
            "message": format!("VM for domain '{domain}' stopped"),
            "domain": domain
        })),
    )
        .into_response()
}

/// POST /shutdown/all — stop every tracked VM (ops only). Server keeps running.
pub async fn shutdown_all_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    tracing::info!("[http-shutdown] Received shutdown/all request");
    let _update_guard = state.lock_update().await;
    let machines = state.take_all_machines().await;
    for (info, machine) in machines {
        tracing::info!(
            "[http-shutdown] Shutting down {} ({})",
            info.id,
            info.domain
        );
        machine.shutdown().await.ok();
    }
    // Belt-and-suspenders: reap any orphan qemu listed under runs/ that may
    // have escaped tracking (racing create, prior crash). Scoped to this
    // XDG data tree — not a host-wide pkill.
    vm_cleanup::reap_qemu_from_runs().await;
    vm_cleanup::sweep_stale_runs().await;
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "ok",
            "message": "All VMs stopped"
        })),
    )
        .into_response()
}

/// Number of trailing lines to send to the client on the first SSE tick.
const INITIAL_TAIL_LINES: usize = 50;

/// Number of trailing line hashes to remember as a content fingerprint for
/// resuming after sliding-window fetches like `journalctl -n N` / `tail -N`.
/// A longer fingerprint better disambiguates against periodic repeats
/// (heartbeats, idle pings); 10 lines comfortably exceeds typical repeat runs.
const RESUME_FINGERPRINT_LINES: usize = 10;

/// Cursor that tracks the trailing content of one log section so we can
/// resume after the next fetch without re-emitting already-streamed lines.
///
/// The data source (`journalctl -n 100`, `tail -100 ...`) returns a sliding
/// window of the most recent lines, NOT an append-only stream, so position-
/// based cursors are unsafe: as new lines arrive, the entire window shifts
/// and any "line at index N" identity is lost. Instead we record a hash
/// fingerprint of the last few lines we saw, then on the next tick locate
/// that fingerprint inside the new window and emit only what follows it.
#[derive(Default)]
struct LogCursor {
    /// Hashes of the last `RESUME_FINGERPRINT_LINES` lines from the previous
    /// fetch (oldest first). Empty before the first non-empty fetch.
    fingerprint: Vec<u64>,
}

impl LogCursor {
    /// Return the slice of `lines` that is new since the last call and
    /// advance the fingerprint to the current tail.
    fn advance<'a>(&mut self, lines: &'a [&'a str]) -> &'a [&'a str] {
        if lines.is_empty() {
            return lines;
        }
        let start = if self.fingerprint.is_empty() {
            // First non-empty fetch: show recent activity without spamming.
            lines.len().saturating_sub(INITIAL_TAIL_LINES)
        } else {
            // Resume right after the previous tail. If the source rolled past our
            // fingerprint (burst faster than the poll window), emit a recent tail
            // so the stream stays live instead of going silent until the burst ends.
            self.find_resume_index(lines)
                .unwrap_or_else(|| lines.len().saturating_sub(INITIAL_TAIL_LINES))
        };

        self.refresh_fingerprint(lines);
        &lines[start.min(lines.len())..]
    }

    /// Locate the index in `lines` immediately after the previously-seen
    /// trailing window. Tries the longest fingerprint suffix first so that
    /// when the source produces repeated identical lines (e.g. heartbeats),
    /// surrounding context disambiguates which occurrence is "ours".
    fn find_resume_index(&self, lines: &[&str]) -> Option<usize> {
        let line_hashes: Vec<u64> = lines.iter().map(|l| hash_line(l)).collect();
        let k = self.fingerprint.len();
        for window in (1..=k).rev() {
            let fp_suffix = &self.fingerprint[k - window..];
            for end in (window..=line_hashes.len()).rev() {
                if line_hashes[end - window..end] == *fp_suffix {
                    return Some(end);
                }
            }
        }
        None
    }

    fn refresh_fingerprint(&mut self, lines: &[&str]) {
        self.fingerprint.clear();
        let start = lines.len().saturating_sub(RESUME_FINGERPRINT_LINES);
        self.fingerprint
            .extend(lines[start..].iter().map(|l| hash_line(l)));
    }
}

fn hash_line(line: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    line.hash(&mut hasher);
    hasher.finish()
}

/// GET /logs/orion/stream - SSE stream for real-time log viewing.
///
/// First tick sends the last `INITIAL_TAIL_LINES` lines, then only newly
/// appended lines on each subsequent tick.
/// Multi-VM: pass `?domain=` or `?vm_id=` to select which runner's logs to stream.
///
/// While the selected VM is still provisioning (no machine handle yet), the
/// stream emits a single waiting line instead of repeating "No running VM"
/// errors every tick.
pub async fn logs_stream_handler(
    State(state): State<Arc<AppState>>,
    Query(q): Query<VmSelectQuery>,
) -> Sse<impl futures_util::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let key = q.domain.or(q.vm_id);
    let stream = async_stream::stream! {
        let mut ticker = interval(std::time::Duration::from_secs(1));
        let mut journal_cursor = LogCursor::default();
        let mut orion_log_offset: u64 = 0;
        let mut waiting_announced = false;
        let mut failure_announced = false;

        loop {
            ticker.tick().await;

            let snapshot = match orion_deployer::get_live_logs_since(
                &state,
                key.as_deref(),
                orion_log_offset,
            )
            .await
            {
                Ok(snapshot) => {
                    waiting_announced = false;
                    failure_announced = false;
                    snapshot
                }
                Err(e) => {
                    let msg = e.to_string();
                    if is_vm_not_ready_error(&msg) {
                        match orion_deployer::get_status_by_key(&state, key.as_deref()).await {
                            Some(vm) if vm.phase == VmPhase::Provisioning => {
                                if !waiting_announced {
                                    waiting_announced = true;
                                    yield Ok(Event::default().data(format!(
                                        "Waiting for VM {} to finish provisioning…",
                                        vm.id
                                    )));
                                }
                            }
                            Some(vm) if vm.phase == VmPhase::Failed => {
                                if !failure_announced {
                                    failure_announced = true;
                                    let detail = vm
                                        .error
                                        .unwrap_or_else(|| "unknown error".to_string());
                                    yield Ok(Event::default().data(format!(
                                        "VM {} failed: {}",
                                        vm.id, detail
                                    )));
                                }
                            }
                            Some(_) | None => {
                                if !waiting_announced {
                                    waiting_announced = true;
                                    yield Ok(Event::default().data(
                                        "Waiting for VM to become available…",
                                    ));
                                }
                            }
                        }
                        continue;
                    }

                    yield Ok(Event::default().data(format!("Error: {}", e)));
                    continue;
                }
            };
            orion_log_offset = snapshot.orion_log_offset;

            let journal_lines: Vec<&str> = snapshot.journal_window.lines().collect();
            let new_j = journal_cursor.advance(&journal_lines);
            let orion_lines: Vec<&str> = snapshot.orion_log_delta.lines().collect();

            if new_j.is_empty() && orion_lines.is_empty() {
                continue;
            }

            let mut output = String::new();
            if !new_j.is_empty() {
                append_logs_section(&mut output, "SYSTEM LOGS", new_j);
            }
            if !orion_lines.is_empty() {
                append_logs_section(&mut output, "ORION LOGS", &orion_lines);
            }

            yield Ok(Event::default().comment("---").data(output));
        }
    };

    Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default())
}

fn is_vm_not_ready_error(msg: &str) -> bool {
    msg.contains("No running VM for key")
        || msg.contains("No VM is currently running")
        || msg.contains("No VM machine handle available")
}

/// Append a log section with a title header and colored log lines to `output`.
fn append_logs_section(output: &mut String, title: &str, lines: &[&str]) {
    use std::fmt::Write;
    let mut wrote_any = false;
    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() || is_noisy_orion_log_line(trimmed) {
            continue;
        }
        if !wrote_any {
            let _ = writeln!(output, "\n─── {} ───", title);
            wrote_any = true;
        }
        output.push_str(&format_log_line(trimmed));
        output.push('\n');
    }
}

/// Drop high-frequency routine lines that drown out useful startup/build output.
fn is_noisy_orion_log_line(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.contains("sending heartbeat") || lower.contains("orion::ws: sending heartbeat")
}

/// GET /vms/{id}/terminal — interactive PTY over WebSocket (vm_id or domain).
///
/// Protocol (matches moon VmTerminal):
/// - client → server: binary = stdin; text JSON `{"type":"resize","cols":N,"rows":N}`
/// - server → client: binary = PTY stdout
pub async fn terminal_ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_terminal_socket(socket, state, id))
}

async fn handle_terminal_socket(socket: WebSocket, state: Arc<AppState>, id: String) {
    let (mut ws_tx, mut ws_rx) = socket.split();

    let _ = ws_tx
        .send(Message::Text("Opening interactive shell…".into()))
        .await;

    let machine = match orion_deployer::resolve_machine_for_handlers(&state, Some(&id)).await {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!("[terminal] resolve VM '{}': {}", id, e);
            send_terminal_fatal(&mut ws_tx, format!("no VM: {e}")).await;
            return;
        }
    };

    let shell = match machine.open_interactive_shell(80, 24).await {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("[terminal] open shell for '{}': {}", id, e);
            send_terminal_fatal(&mut ws_tx, format!("shell: {e}")).await;
            return;
        }
    };

    let (mut reader, writer) = shell.split();
    let _ = ws_tx.send(Message::Text("Shell ready".into())).await;
    tracing::info!("[terminal] session opened for '{}'", id);

    loop {
        tokio::select! {
            msg = ws_rx.next() => {
                match msg {
                    Some(Ok(Message::Binary(data))) => {
                        if let Err(e) = writer.write(&data).await {
                            tracing::warn!("[terminal] write: {}", e);
                            break;
                        }
                    }
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<TerminalResizeMsg>(&text) {
                            Ok(msg) if msg.msg_type == "resize" => {
                                if let Err(e) = writer.resize(msg.cols, msg.rows).await {
                                    tracing::warn!("[terminal] resize: {}", e);
                                    break;
                                }
                            }
                            _ => {}
                        }
                    }
                    Some(Ok(Message::Ping(p))) => {
                        let _ = ws_tx.send(Message::Pong(p)).await;
                    }
                    Some(Ok(Message::Pong(_))) => {}
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(e)) => {
                        tracing::warn!("[terminal] ws recv: {}", e);
                        break;
                    }
                }
            }
            chunk = reader.read_chunk() => {
                match chunk {
                    Ok(Some(data)) => {
                        if ws_tx.send(Message::Binary(data.into())).await.is_err() {
                            break;
                        }
                    }
                    Ok(None) => {
                        tracing::info!("[terminal] shell exited for '{}'", id);
                        break;
                    }
                    Err(e) => {
                        tracing::warn!("[terminal] read_chunk: {}", e);
                        break;
                    }
                }
            }
        }
    }

    let _ = writer.close(reader).await;
    let _ = ws_tx.send(Message::Close(None)).await;
    tracing::info!("[terminal] session closed for '{}'", id);
}

#[derive(Deserialize)]
struct TerminalResizeMsg {
    #[serde(rename = "type")]
    msg_type: String,
    cols: u32,
    rows: u32,
}

fn truncate_close_reason(s: String) -> String {
    const MAX: usize = 120;
    if s.len() <= MAX {
        s
    } else {
        format!("{}…", &s[..MAX.saturating_sub(1)])
    }
}

/// Send a fatal terminal error as text (visible through mono proxy) then close.
async fn send_terminal_fatal(
    ws_tx: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    message: String,
) {
    let _ = ws_tx
        .send(Message::Text(format!("Error: {message}").into()))
        .await;
    let _ = ws_tx
        .send(Message::Close(Some(axum::extract::ws::CloseFrame {
            code: axum::extract::ws::close_code::ERROR,
            reason: truncate_close_reason(message).into(),
        })))
        .await;
}
