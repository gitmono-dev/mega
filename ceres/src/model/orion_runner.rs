use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Serialize, Deserialize, ToSchema, Debug, Clone, Default)]
pub struct StartRunnerRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_disk_gb: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_cpus: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_memory_mb: Option<u32>,
}

#[derive(Serialize, Deserialize, ToSchema, Debug, Clone)]
pub struct StartRunnerResponse {
    pub vm_id: String,
    pub phase: String,
}

#[derive(Serialize, Deserialize, ToSchema, Debug, Clone)]
pub struct RunnerStatusResponse {
    pub vm_id: String,
    pub phase: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vm_ip: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uptime_secs: Option<u64>,
}
