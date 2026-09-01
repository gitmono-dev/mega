use api_model::common::CommonResult;
use axum::{
    Json,
    extract::{Path, State},
};
use ceres::model::{
    code_review::{
        CodeReviewResponse, CommentReplyRequest, CommentReviewResponse, InitializeCommentRequest,
        ThreadReviewResponse, ThreadStatusResponse, UpdateCommentRequest,
    },
    serde_snowflake::SnowflakeId,
};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::api::{
    MonoApiServiceState, api_common::identity::collaboration_actor, api_doc::CODE_REVIEW_TAG,
    error::ApiError, oauth::model::LoginUser,
};

pub fn routers() -> OpenApiRouter<MonoApiServiceState> {
    OpenApiRouter::new().nest(
        "/code_review",
        OpenApiRouter::new()
            .routes(routes!(code_review_comment_list))
            .routes(routes!(initialize_code_review_comment))
            .routes(routes!(reply_code_review_comment))
            .routes(routes!(update_code_review_comment))
            .routes(routes!(resolve_code_review_thread))
            .routes(routes!(reopen_code_review_thread))
            .routes(routes!(delete_code_review_thread))
            .routes(routes!(delete_code_review_comment)),
    )
}

/// List code review comments
#[utoipa::path(
    get,
    params(
        ("link", description = "CL link"),
    ),
    path = "/{link}/comments",
    responses(
        (status = 200, body = CommonResult<CodeReviewResponse>, content_type = "application/json")
    ),
    tag = CODE_REVIEW_TAG,
)]
async fn code_review_comment_list(
    Path(link): Path<String>,
    state: State<MonoApiServiceState>,
) -> Result<Json<CommonResult<CodeReviewResponse>>, ApiError> {
    let comments = state
        .services()
        .code_review()
        .get_code_review_comments(&link)
        .await?;

    Ok(Json(CommonResult::success(Some(comments))))
}

/// Initialize a code review comment in a new thread
#[utoipa::path(
    post,
    params(
        ("link", description = "CL link"),
    ),
    path = "/{link}/comment/init",
    responses(
        (status = 200, body = CommonResult<ThreadReviewResponse>, content_type = "application/json")
    ),
    tag = CODE_REVIEW_TAG,
)]
async fn initialize_code_review_comment(
    user: LoginUser,
    Path(link): Path<String>,
    state: State<MonoApiServiceState>,
    Json(paload): Json<InitializeCommentRequest>,
) -> Result<Json<CommonResult<ThreadReviewResponse>>, ApiError> {
    let actor = collaboration_actor(&user)?;
    let thread = state
        .services()
        .code_review()
        .create_code_review_comment(&link, actor.to_string(), paload)
        .await?;

    Ok(Json(CommonResult::success(Some(thread))))
}

/// Reply to a code review comment
#[utoipa::path(
    post,
    params(
        ("thread_id" = SnowflakeId, Path, description = "Code Review Comment Thread ID"),
    ),
    path = "/{thread_id}/comment/reply",
    responses(
        (status = 200, body = CommonResult<CommentReviewResponse>, content_type = "application/json")
    ),
    tag = CODE_REVIEW_TAG,
)]
async fn reply_code_review_comment(
    user: LoginUser,
    Path(SnowflakeId(thread_id)): Path<SnowflakeId>,
    state: State<MonoApiServiceState>,
    Json(payload): Json<CommentReplyRequest>,
) -> Result<Json<CommonResult<CommentReviewResponse>>, ApiError> {
    let actor = collaboration_actor(&user)?;
    let comment = state
        .services()
        .code_review()
        .reply_code_review_comment(thread_id, actor.to_string(), payload)
        .await?;

    Ok(Json(CommonResult::success(Some(comment))))
}

/// Update a code review comment
#[utoipa::path(
    post,
    params(
        ("comment_id" = SnowflakeId, Path, description = "A numeric ID representing a comment"),
    ),
    path = "/{comment_id}/update",
    responses(
        (status = 200, body = CommonResult<CommentReviewResponse>, content_type = "application/json")
    ),
    tag = CODE_REVIEW_TAG,
)]
async fn update_code_review_comment(
    user: LoginUser,
    Path(SnowflakeId(comment_id)): Path<SnowflakeId>,
    state: State<MonoApiServiceState>,
    Json(payload): Json<UpdateCommentRequest>,
) -> Result<Json<CommonResult<CommentReviewResponse>>, ApiError> {
    let actor = collaboration_actor(&user)?;
    let comment = state
        .services()
        .code_review()
        .update_code_review_comment(comment_id, actor, payload)
        .await?;

    Ok(Json(CommonResult::success(Some(comment))))
}

/// Resolve a code review thread
#[utoipa::path(
    post,
    params(
        ("thread_id" = SnowflakeId, Path, description = "A numeric ID representing a code review thread"),
    ),
    path = "/{thread_id}/resolve",
    responses(
        (status = 200, body = CommonResult<ThreadStatusResponse>, content_type = "application/json")
    ),
    tag = CODE_REVIEW_TAG,
)]
async fn resolve_code_review_thread(
    Path(SnowflakeId(thread_id)): Path<SnowflakeId>,
    state: State<MonoApiServiceState>,
) -> Result<Json<CommonResult<ThreadStatusResponse>>, ApiError> {
    let thread = state
        .services()
        .code_review()
        .resolve_code_review_thread(thread_id)
        .await?;

    Ok(Json(CommonResult::success(Some(thread))))
}

/// Reopen a code review thread
#[utoipa::path(
    post,
    params(
        ("thread_id" = SnowflakeId, Path, description = "A numeric ID representing a code review thread"),
    ),
    path = "/{thread_id}/reopen",
    responses(
        (status = 200, body = CommonResult<ThreadStatusResponse>, content_type = "application/json")
    ),
    tag = CODE_REVIEW_TAG,
)]
async fn reopen_code_review_thread(
    Path(SnowflakeId(thread_id)): Path<SnowflakeId>,
    state: State<MonoApiServiceState>,
) -> Result<Json<CommonResult<ThreadStatusResponse>>, ApiError> {
    let thread = state
        .services()
        .code_review()
        .reopen_code_review_thread(thread_id)
        .await?;

    Ok(Json(CommonResult::success(Some(thread))))
}

/// Delete a code review thread and its comments
#[utoipa::path(
    delete,
    params(
        ("thread_id" = SnowflakeId, Path, description = "A numeric ID representing a code review thread"),
    ),
    path = "/thread/{thread_id}",
    responses(
        (status = 200, body = CommonResult<String>, content_type = "application/json")
    ),
    tag = CODE_REVIEW_TAG,
)]
async fn delete_code_review_thread(
    Path(SnowflakeId(thread_id)): Path<SnowflakeId>,
    state: State<MonoApiServiceState>,
) -> Result<Json<CommonResult<String>>, ApiError> {
    state
        .services()
        .code_review()
        .delete_code_review_thread(thread_id)
        .await?;

    Ok(Json(CommonResult::success(None)))
}

/// Delete a code review comment
#[utoipa::path(
    delete,
    params(
        ("comment_id" = SnowflakeId, Path, description = "A numeric ID representing a code review comment"),
    ),
    path = "/comment/{comment_id}",
    responses(
        (status = 200, body = CommonResult<String>, content_type = "application/json")
    ),
    tag = CODE_REVIEW_TAG,
)]
async fn delete_code_review_comment(
    user: LoginUser,
    Path(SnowflakeId(comment_id)): Path<SnowflakeId>,
    state: State<MonoApiServiceState>,
) -> Result<Json<CommonResult<String>>, ApiError> {
    let actor = collaboration_actor(&user)?;
    state
        .services()
        .code_review()
        .delete_code_review_comment(comment_id, actor)
        .await?;

    Ok(Json(CommonResult::success(None)))
}
