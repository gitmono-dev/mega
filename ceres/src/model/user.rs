use callisto::{access_token, ssh_keys};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

#[derive(Debug, Deserialize, ToSchema)]
pub struct AddSSHKey {
    pub title: String,
    pub ssh_key: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ListSSHKey {
    pub id: i64,
    pub title: String,
    pub ssh_key: String,
    pub finger: String,
    pub created_at: i64,
}

impl From<ssh_keys::Model> for ListSSHKey {
    fn from(value: ssh_keys::Model) -> Self {
        Self {
            id: value.id,
            title: value.title,
            ssh_key: value.ssh_key,
            finger: value.finger,
            created_at: value.created_at.and_utc().timestamp(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ListToken {
    pub id: i64,
    pub token: String,
    pub created_at: i64,
}

impl From<access_token::Model> for ListToken {
    fn from(value: access_token::Model) -> Self {
        let mut mask_token = value.token;
        mask_token.replace_range(7..32, "-******-");
        Self {
            id: value.id,
            token: mask_token,
            created_at: value.created_at.and_utc().timestamp(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RepoPermissions {
    pub admin: Vec<String>,
    pub maintainer: Vec<String>,
    pub reader: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ClaSignStatusRes {
    pub username: String,
    pub cla_signed: bool,
    pub cla_signed_at: Option<i64>,
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct UpdateClaContentPayload {
    pub content: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ClaContentRes {
    pub content: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct UserApprovalStatusRes {
    pub username: String,
    pub campsite_user_id: String,
    pub display_name: String,
    pub email: String,
    /// One of: pending, approved, rejected
    pub status: String,
    pub reviewed_by: Option<String>,
    /// Unix timestamp
    pub reviewed_at: Option<i64>,
    /// Unix timestamp
    pub registered_at: i64,
}

#[derive(Debug, Deserialize, IntoParams, ToSchema)]
pub struct ListUserApprovalsQuery {
    /// Filter by status: pending, approved, rejected, or all (default: pending)
    pub status: Option<String>,
    /// Max rows to return (default 100, max 500)
    pub limit: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct UserApprovalListRes {
    pub items: Vec<UserApprovalStatusRes>,
}
