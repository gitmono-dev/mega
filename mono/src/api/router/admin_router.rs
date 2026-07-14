//! Admin-related API endpoints.
//!
//! Provides endpoints for admin permission checks and user account approval:
//! - `GET /api/v1/admin/me` - Check if current user is admin
//! - `GET /api/v1/admin/list` - List all admins (admin-only)
//! - `GET /api/v1/admin/user-approvals` - List user approval records (admin-only)
//! - `POST /api/v1/admin/user-approvals/{username}/approve` - Approve a user (admin-only)
//! - `POST /api/v1/admin/user-approvals/{username}/reject` - Reject a user (admin-only)
//!
//! # Auth Behavior
//! - 401 Unauthorized: No valid session (handled by `LoginUser` extractor)
//! - 403 Forbidden: Logged in but not admin (for admin-only endpoints)

use api_model::common::CommonResult;
use axum::{
    Json,
    extract::{Path, Query, State},
};
use ceres::model::{
    admin::{AdminListResponse, IsAdminResponse},
    user::{ListUserApprovalsQuery, UserApprovalListRes, UserApprovalStatusRes},
};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::api::{
    MonoApiServiceState, api_common::group_permission::ensure_admin, api_doc::USER_TAG,
    error::ApiError, oauth::model::LoginUser,
};

/// Build the admin router.
pub fn routers() -> OpenApiRouter<MonoApiServiceState> {
    OpenApiRouter::new().nest(
        "/admin",
        OpenApiRouter::new()
            .routes(routes!(is_admin_me))
            .routes(routes!(admin_list))
            .routes(routes!(list_user_approvals))
            .routes(routes!(approve_user))
            .routes(routes!(reject_user)),
    )
}

/// GET /api/v1/admin/me
///
/// Returns whether the current user is an admin.
#[utoipa::path(
    get,
    path = "/me",
    responses(
        (status = 200, body = CommonResult<IsAdminResponse>),
        (status = 401, description = "Unauthorized"),
    ),
    tag = USER_TAG
)]
async fn is_admin_me(
    user: LoginUser,
    State(state): State<MonoApiServiceState>,
) -> Result<Json<CommonResult<IsAdminResponse>>, ApiError> {
    let is_admin = state
        .services()
        .admin()
        .check_is_admin(&user.username)
        .await?;

    Ok(Json(CommonResult::success(Some(IsAdminResponse {
        is_admin,
    }))))
}

/// GET /api/v1/admin/list
///
/// Returns a list of all admin usernames.
/// Only admins can access this endpoint.
#[utoipa::path(
    get,
    path = "/list",
    responses(
        (status = 200, body = CommonResult<AdminListResponse>),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden - not admin"),
    ),
    tag = USER_TAG
)]
async fn admin_list(
    user: LoginUser,
    State(state): State<MonoApiServiceState>,
) -> Result<Json<CommonResult<AdminListResponse>>, ApiError> {
    ensure_admin(&state, &user).await?;

    let admins = state.services().admin().get_all_admins().await?;

    Ok(Json(CommonResult::success(Some(AdminListResponse {
        admins,
    }))))
}

/// GET /api/v1/admin/user-approvals
///
/// List user account approval records. Defaults to pending.
#[utoipa::path(
    get,
    path = "/user-approvals",
    params(ListUserApprovalsQuery),
    responses(
        (status = 200, body = CommonResult<UserApprovalListRes>),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden - not admin"),
    ),
    tag = USER_TAG
)]
async fn list_user_approvals(
    user: LoginUser,
    State(state): State<MonoApiServiceState>,
    Query(query): Query<ListUserApprovalsQuery>,
) -> Result<Json<CommonResult<UserApprovalListRes>>, ApiError> {
    ensure_admin(&state, &user).await?;

    let status = query.status.as_deref().unwrap_or("pending");
    let limit = query.limit.unwrap_or(100).min(500);

    let items = state
        .services()
        .user()
        .list_user_approvals(Some(status), limit)
        .await?
        .into_iter()
        .map(UserApprovalStatusRes::from)
        .collect();

    Ok(Json(CommonResult::success(Some(UserApprovalListRes {
        items,
    }))))
}

/// POST /api/v1/admin/user-approvals/{username}/approve
#[utoipa::path(
    post,
    path = "/user-approvals/{username}/approve",
    params(
        ("username" = String, Path, description = "Username to approve")
    ),
    responses(
        (status = 200, body = CommonResult<UserApprovalStatusRes>),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden - not admin"),
    ),
    tag = USER_TAG
)]
async fn approve_user(
    user: LoginUser,
    State(state): State<MonoApiServiceState>,
    Path(username): Path<String>,
) -> Result<Json<CommonResult<UserApprovalStatusRes>>, ApiError> {
    ensure_admin(&state, &user).await?;

    let model = state
        .services()
        .user()
        .approve_user(&username, &user.username)
        .await?;

    Ok(Json(CommonResult::success(Some(
        UserApprovalStatusRes::from(model),
    ))))
}

/// POST /api/v1/admin/user-approvals/{username}/reject
#[utoipa::path(
    post,
    path = "/user-approvals/{username}/reject",
    params(
        ("username" = String, Path, description = "Username to reject")
    ),
    responses(
        (status = 200, body = CommonResult<UserApprovalStatusRes>),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden - not admin"),
    ),
    tag = USER_TAG
)]
async fn reject_user(
    user: LoginUser,
    State(state): State<MonoApiServiceState>,
    Path(username): Path<String>,
) -> Result<Json<CommonResult<UserApprovalStatusRes>>, ApiError> {
    ensure_admin(&state, &user).await?;

    let model = state
        .services()
        .user()
        .reject_user(&username, &user.username)
        .await?;

    Ok(Json(CommonResult::success(Some(
        UserApprovalStatusRes::from(model),
    ))))
}
