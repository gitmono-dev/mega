use anyhow::anyhow;
use ceres::{
    application::api_service::mono::EffectiveResourcePermission,
    model::group::{PermissionValue, ResourceTypeValue, UserEffectivePermissionResponse},
};
use http::StatusCode;

use crate::api::{MonoApiServiceState, error::ApiError, oauth::model::LoginUser};

pub async fn ensure_admin(state: &MonoApiServiceState, user: &LoginUser) -> Result<(), ApiError> {
    let admin = state.services().admin();
    let cedar_id = user.cedar_user_id();

    // Prefer GitHub login (cedar_user_id). Also accept Campsite username while
    // `.mega_cedar.json` is still migrating from username → github_login.
    if admin.check_is_admin(cedar_id).await?
        || (cedar_id != user.username.as_str() && admin.check_is_admin(&user.username).await?)
    {
        return Ok(());
    }

    tracing::warn!(
        actor = %user.username,
        cedar_user_id = %cedar_id,
        github_login = ?user.github_login,
        "admin check failed: access forbidden"
    );

    Err(ApiError::with_status(
        StatusCode::FORBIDDEN,
        anyhow!("Admin access required"),
    ))
}

pub async fn resolve_resource_context(
    state: &MonoApiServiceState,
    resource_type: &str,
    resource_id: &str,
) -> Result<(ResourceTypeValue, String), ApiError> {
    state
        .services()
        .admin()
        .resolve_resource_context(resource_type, resource_id)
        .await
        .map_err(ApiError::from)
}

pub fn build_user_effective_permission_response(
    username: String,
    resource_type: ResourceTypeValue,
    resource_id: String,
    effective: EffectiveResourcePermission,
) -> UserEffectivePermissionResponse {
    let permission: Option<PermissionValue> = effective.permission.map(Into::into);

    UserEffectivePermissionResponse {
        username,
        resource_type,
        resource_id,
        is_admin: effective.is_admin,
        permission,
        has_read: effective.is_admin || has_permission(permission, PermissionValue::Read),
        has_write: effective.is_admin || has_permission(permission, PermissionValue::Write),
        has_admin: effective.is_admin || has_permission(permission, PermissionValue::Admin),
    }
}

fn has_permission(current: Option<PermissionValue>, required: PermissionValue) -> bool {
    current
        .map(|value| value.satisfies(required))
        .unwrap_or(false)
}
