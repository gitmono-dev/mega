//! Build Trigger API v1 Router (RESTful)
//!
//! Provides RESTful endpoints for creating and managing build triggers.
//! This is the new API design that follows industry best practices.

use api_model::common::{CommonPage, CommonResult, PageParams};
use axum::{
    Json,
    extract::{Path, State},
};
use ceres::{
    application::build_trigger::{CreateTriggerRequest, ListTriggersParams, TriggerResponse},
    model::serde_snowflake::SnowflakeId,
};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::api::{
    MonoApiServiceState, api_common::identity::collaboration_actor, api_doc::BUILD_TRIGGER_TAG,
    error::ApiError, oauth::model::LoginUser,
};

pub fn routers() -> OpenApiRouter<MonoApiServiceState> {
    OpenApiRouter::new().nest(
        "/triggers",
        OpenApiRouter::new()
            .routes(routes!(create_trigger))
            .routes(routes!(list_triggers))
            .routes(routes!(get_trigger))
            .routes(routes!(retry_trigger)),
    )
}

/// Create a new build trigger
///
/// Creates a new build trigger with automatic ref resolution.
/// Supports branch names, tag names, commit hashes, or CL links.
/// Defaults to "main" branch if no ref is specified.
#[utoipa::path(
    post,
    path = "",
    request_body = CreateTriggerRequest,
    responses(
        (status = 200, body = CommonResult<TriggerResponse>, content_type = "application/json"),
        (status = 400, description = "Invalid request parameters or ref not found"),
        (status = 503, description = "Build system not enabled")
    ),
    tag = BUILD_TRIGGER_TAG
)]
async fn create_trigger(
    user: LoginUser,
    state: State<MonoApiServiceState>,
    Json(req): Json<CreateTriggerRequest>,
) -> Result<Json<CommonResult<TriggerResponse>>, ApiError> {
    let actor = collaboration_actor(&user)?;
    let service = state.services().build_trigger();
    let response = service
        .create_manual_trigger(req.repo_path, req.ref_name, req.params, actor.to_string())
        .await?;
    Ok(Json(CommonResult::success(Some(response))))
}

/// List build triggers with filters
///
/// Returns build triggers with pagination and optional filters.
/// Supports filtering by repository, trigger type, source, user, and time range.
///
/// This endpoint follows the project's standard Google-style API pattern:
/// - Uses POST method for complex query parameters
/// - Accepts PageParams with pagination and filter parameters
/// - Returns CommonPage with items and total count
#[utoipa::path(
    post,
    path = "/list",
    request_body = PageParams<ListTriggersParams>,
    responses(
        (status = 200, body = CommonResult<CommonPage<TriggerResponse>>, content_type = "application/json"),
        (status = 503, description = "Build system not enabled")
    ),
    tag = BUILD_TRIGGER_TAG
)]
async fn list_triggers(
    _user: LoginUser,
    state: State<MonoApiServiceState>,
    Json(json): Json<PageParams<ListTriggersParams>>,
) -> Result<Json<CommonResult<CommonPage<TriggerResponse>>>, ApiError> {
    let service = state.services().build_trigger();
    let (items, total) = service
        .list_triggers(json.additional, json.pagination)
        .await?;
    Ok(Json(CommonResult::success(Some(CommonPage {
        items,
        total: total as u64,
    }))))
}

/// Get a specific build trigger by ID
///
/// Returns complete details about a specific trigger including:
/// - Trigger metadata (type, source, time)
/// - Repository and commit information
/// - Ref information (branch/tag name if applicable)
/// - Build parameters
#[utoipa::path(
    get,
    path = "/{id}",
    params(
        ("id" = SnowflakeId, Path, description = "Trigger ID")
    ),
    responses(
        (status = 200, body = CommonResult<TriggerResponse>, content_type = "application/json"),
        (status = 404, description = "Trigger not found"),
        (status = 503, description = "Build system not enabled")
    ),
    tag = BUILD_TRIGGER_TAG
)]
async fn get_trigger(
    _user: LoginUser,
    state: State<MonoApiServiceState>,
    Path(SnowflakeId(id)): Path<SnowflakeId>,
) -> Result<Json<CommonResult<TriggerResponse>>, ApiError> {
    let service = state.services().build_trigger();
    let response = service.get_trigger(id).await?;
    Ok(Json(CommonResult::success(Some(response))))
}

/// Retry a specific build trigger
///
/// Creates a new trigger that retries a previous build.
/// The new trigger will use the same repository, commit, and parameters
/// as the original trigger.
#[utoipa::path(
    post,
    path = "/{id}/retry",
    params(
        ("id" = SnowflakeId, Path, description = "Original trigger ID to retry")
    ),
    responses(
        (status = 200, body = CommonResult<TriggerResponse>, content_type = "application/json"),
        (status = 404, description = "Original trigger not found"),
        (status = 503, description = "Build system not enabled")
    ),
    tag = BUILD_TRIGGER_TAG
)]
async fn retry_trigger(
    user: LoginUser,
    state: State<MonoApiServiceState>,
    Path(SnowflakeId(id)): Path<SnowflakeId>,
) -> Result<Json<CommonResult<TriggerResponse>>, ApiError> {
    let actor = collaboration_actor(&user)?;
    let service = state.services().build_trigger();
    let response = service.retry_trigger(id, actor.to_string()).await?;
    Ok(Json(CommonResult::success(Some(response))))
}
