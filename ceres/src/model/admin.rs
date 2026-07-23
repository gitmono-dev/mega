use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub struct IsAdminResponse {
    pub is_admin: bool,
}

#[derive(Serialize, ToSchema)]
pub struct AdminListResponse {
    pub admins: Vec<String>,
}

/// Request body for generating `.mega_cedar.json` content from admin GitHub logins.
#[derive(Debug, Deserialize, ToSchema)]
pub struct GenerateCedarRequest {
    /// GitHub login names used as Cedar `User` euids (e.g. `octocat`).
    pub admins: Vec<String>,
}

/// Response containing generated `.mega_cedar.json` content.
#[derive(Serialize, ToSchema)]
pub struct GenerateCedarResponse {
    pub content: String,
}
