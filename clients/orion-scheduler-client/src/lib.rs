//! HTTP client for orion-scheduler VM provisioning (`/webhook`, `/status`, `/vms/{id}`, logs SSE).

mod http_client;

use common::config::BuildConfig;
pub use http_client::OrionSchedulerHttpClient;
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
    pub vm_ip: Option<String>,
    #[serde(default)]
    pub uptime_secs: Option<u64>,
    #[serde(default)]
    pub log_file: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
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

    /// Backward-compatible list/status endpoint.
    pub async fn get_status(&self) -> anyhow::Result<SchedulerStatusResponse> {
        self.http.get_status().await
    }

    /// Proxy-friendly SSE stream of runner / orion-client startup logs.
    pub async fn stream_orion_logs(
        &self,
        vm_id: Option<&str>,
        domain: Option<&str>,
    ) -> anyhow::Result<reqwest::Response> {
        self.http.stream_orion_logs(vm_id, domain).await
    }
}
