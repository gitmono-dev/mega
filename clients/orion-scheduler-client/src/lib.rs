//! HTTP client for orion-scheduler VM provisioning (`/webhook`, `/status`, `/vms/{id}`, logs SSE, terminal WS).

mod http_client;

use common::config::BuildConfig;
pub use http_client::{OrionSchedulerHttpClient, TerminalWebSocket};
use serde::{Deserialize, Serialize};

/// Request body for starting a runner VM via scheduler `/webhook`.
#[derive(Debug, Clone, Serialize)]
pub struct StartRunnerPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub replace: bool,
    pub server_ws: String,
    pub scorpio_base_url: String,
    pub scorpio_lfs_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_digest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_disk_gb: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_cpus: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_memory_mb: Option<u32>,
    /// When set, write `ORION_RETAIN_ANTARES_MOUNTS` into the guest `.env`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retain_antares_mounts: Option<bool>,
}

/// Response from scheduler `POST /webhook` (async 202, sync 200, conflict 409).
#[derive(Debug, Clone, Deserialize)]
pub struct StartRunnerSchedulerResponse {
    pub status: String,
    pub vm_id: Option<String>,
    #[serde(default)]
    pub domain: Option<String>,
    #[serde(default)]
    pub phase: Option<String>,
    pub error: Option<String>,
    #[serde(default)]
    pub orion_log_file: Option<String>,
}

/// Response from scheduler `GET /vms/{id}` or filtered `/status`.
#[derive(Debug, Clone, Deserialize)]
pub struct SchedulerStatusResponse {
    pub status: String,
    #[serde(default)]
    pub phase: Option<String>,
    pub vm_id: Option<String>,
    #[serde(default)]
    pub domain: Option<String>,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub vm_ip: Option<String>,
    #[serde(default)]
    pub uptime_secs: Option<u64>,
    #[serde(default)]
    pub log_file: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub image_path: Option<String>,
    #[serde(default)]
    pub image_digest: Option<String>,
    #[serde(default)]
    pub image_cpus: Option<u32>,
    #[serde(default)]
    pub image_memory_mb: Option<u32>,
    #[serde(default)]
    pub image_disk_gb: Option<u32>,
    #[serde(default)]
    pub image_name: Option<String>,
    #[serde(default)]
    pub image_built_at: Option<String>,
    #[serde(default)]
    pub toolchain_rust: Option<String>,
    #[serde(default)]
    pub toolchain_buck2: Option<String>,
    #[serde(default)]
    pub toolchain_python: Option<String>,
    #[serde(default)]
    pub kernel: Option<String>,
}

/// Response from scheduler `GET /status` (all VMs).
#[derive(Debug, Clone, Deserialize)]
pub struct SchedulerVmListResponse {
    pub status: String,
    #[serde(default)]
    pub count: usize,
    #[serde(default)]
    pub vms: Vec<SchedulerStatusResponse>,
}

#[derive(Clone)]
pub struct OrionSchedulerClient {
    http: OrionSchedulerHttpClient,
    build_config: BuildConfig,
}

impl OrionSchedulerClient {
    pub fn new(build_config: BuildConfig) -> Self {
        let token = build_config.orion_scheduler_token.clone();
        let http = OrionSchedulerHttpClient::new(build_config.orion_scheduler_url.clone(), token);
        Self { http, build_config }
    }

    pub fn is_configured(&self) -> bool {
        !self.build_config.orion_scheduler_url.trim().is_empty()
    }

    pub async fn start_runner(
        &self,
        payload: StartRunnerPayload,
    ) -> anyhow::Result<StartRunnerSchedulerResponse> {
        self.http.start_runner(payload).await
    }

    pub async fn get_vm_status(&self, vm_id: &str) -> anyhow::Result<SchedulerStatusResponse> {
        self.http.get_vm_status(vm_id).await
    }

    /// Backward-compatible list/status endpoint (returns first VM if any).
    pub async fn get_status(&self) -> anyhow::Result<SchedulerStatusResponse> {
        let list = self.list_vms().await?;
        Ok(list
            .vms
            .into_iter()
            .next()
            .unwrap_or(SchedulerStatusResponse {
                status: "no_vm".to_string(),
                phase: Some("no_vm".to_string()),
                vm_id: None,
                domain: None,
                target: None,
                vm_ip: None,
                uptime_secs: None,
                log_file: None,
                error: None,
                image_path: None,
                image_digest: None,
                image_cpus: None,
                image_memory_mb: None,
                image_disk_gb: None,
                image_name: None,
                image_built_at: None,
                toolchain_rust: None,
                toolchain_buck2: None,
                toolchain_python: None,
                kernel: None,
            }))
    }

    /// List all VMs tracked by the scheduler (`GET /status`).
    pub async fn list_vms(&self) -> anyhow::Result<SchedulerVmListResponse> {
        self.http.list_vms().await
    }

    /// Proxy-friendly SSE stream of runner / orion-client startup logs.
    pub async fn stream_orion_logs(
        &self,
        vm_id: Option<&str>,
        domain: Option<&str>,
    ) -> anyhow::Result<reqwest::Response> {
        self.http.stream_orion_logs(vm_id, domain).await
    }

    /// Open a WebSocket PTY session to a Running VM (`GET /vms/{id}/terminal`).
    pub async fn connect_terminal(&self, id: &str) -> anyhow::Result<TerminalWebSocket> {
        self.http.connect_terminal(id).await
    }
}
