use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Serialize, Deserialize, ToSchema, Debug, Clone)]
pub struct OrionVmImageResponse {
    pub id: String,
    pub digest: String,
    pub object_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub info_object_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub built_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rust: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub buck2: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub python: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kernel: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub created_at: String,
}

#[derive(Serialize, Deserialize, ToSchema, Debug, Clone)]
pub struct OrionVmImageListResponse {
    pub count: usize,
    pub images: Vec<OrionVmImageResponse>,
}

#[derive(Serialize, Deserialize, ToSchema, Debug, Clone)]
pub struct RegisterOrionVmImageRequest {
    /// Content digest, e.g. `sha256:<hex>`.
    pub digest: String,
    /// Key under the `orion-images/` namespace, e.g. `{hex}/debian-13-buck2.qcow2`.
    pub object_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub info_object_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub built_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rust: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub buck2: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub python: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kernel: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Serialize, Deserialize, ToSchema, Debug, Clone)]
pub struct PresignOrionVmImageRequest {
    /// Content digest, e.g. `sha256:<hex>`.
    pub digest: String,
    /// Base name used in the object key (default `debian-13-buck2`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_name: Option<String>,
    /// When true, also return a PUT URL for `{hex}/image-info.json`.
    #[serde(default)]
    pub with_info: bool,
}

#[derive(Serialize, Deserialize, ToSchema, Debug, Clone)]
pub struct PresignOrionVmImageResponse {
    pub object_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub info_object_key: Option<String>,
    pub qcow2_put_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub info_put_url: Option<String>,
    pub expires_in_secs: u64,
}
