use anyhow::anyhow;
use http::StatusCode;

use crate::api::{error::ApiError, oauth::model::LoginUser};

/// Persistent actor id for CL/Issue/account writes: Campsite public user id.
pub fn collaboration_actor(user: &LoginUser) -> Result<&str, ApiError> {
    user.require_campsite_user_id()
        .map_err(|msg| ApiError::with_status(StatusCode::FORBIDDEN, anyhow!(msg)))
}

/// GitHub login for Cedar / display / reviewer.github_login dual column.
pub fn collaboration_github_login(user: &LoginUser) -> Result<&str, ApiError> {
    user.require_github_login()
        .map_err(|msg| ApiError::with_status(StatusCode::FORBIDDEN, anyhow!(msg)))
}
