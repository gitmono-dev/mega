use api_model::common::CommonResult;
use axum::{
    Json,
    extract::{Path, State},
};
use ceres::model::conversation::{ContentPayload, ReactionRequest};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::api::{
    MonoApiServiceState, api_common::identity::collaboration_actor, api_doc::CONV_TAG,
    error::ApiError, oauth::model::LoginUser,
};

pub fn routers() -> OpenApiRouter<MonoApiServiceState> {
    OpenApiRouter::new().nest(
        "/conversation",
        OpenApiRouter::new()
            .routes(routes!(comment_reactions))
            .routes(routes!(delete_comment_reaction))
            .routes(routes!(delete_comment))
            .routes(routes!(edit_comment)),
    )
}

/// Add comment reactions with emoji
#[utoipa::path(
    post,
    params(
        ("comment_id", description = "A numeric ID representing either a comment or a conversation. Specify the type in the request body."),
    ),
    path = "/{comment_id}/reactions",
    request_body = ReactionRequest,
    responses(
        (status = 200, body = CommonResult<String>, content_type = "application/json")
    ),
    tag = CONV_TAG
)]
async fn comment_reactions(
    user: LoginUser,
    Path(comment_id): Path<i64>,
    state: State<MonoApiServiceState>,
    Json(payload): Json<ReactionRequest>,
) -> Result<Json<CommonResult<String>>, ApiError> {
    let actor = collaboration_actor(&user)?;
    state
        .services()
        .conversation()
        .add_comment_reaction(
            Some(payload.content),
            comment_id,
            &payload.comment_type,
            actor,
        )
        .await?;
    Ok(Json(CommonResult::success(None)))
}

/// Delete conversation reactions
#[utoipa::path(
    delete,
    params(
        ("id", description = "viewer_reaction_id"),
    ),
    path = "/reactions/{id}",
    responses(
        (status = 200, body = CommonResult<String>, content_type = "application/json")
    ),
    tag = CONV_TAG
)]
async fn delete_comment_reaction(
    user: LoginUser,
    Path(id): Path<String>,
    state: State<MonoApiServiceState>,
) -> Result<Json<CommonResult<String>>, ApiError> {
    let actor = collaboration_actor(&user)?;
    state
        .services()
        .conversation()
        .delete_comment_reaction(&id, actor)
        .await?;
    Ok(Json(CommonResult::success(None)))
}

/// Delete Comment
#[utoipa::path(
    delete,
    params(
        ("comment_id", description = "A numeric ID representing a comment"),
    ),
    path = "/{comment_id}",
    responses(
        (status = 200, body = CommonResult<String>, content_type = "application/json")
    ),
    tag = CONV_TAG
)]
async fn delete_comment(
    Path(comment_id): Path<i64>,
    state: State<MonoApiServiceState>,
) -> Result<Json<CommonResult<String>>, ApiError> {
    state
        .services()
        .conversation()
        .remove_conversation(comment_id)
        .await?;
    Ok(Json(CommonResult::success(None)))
}

/// Edit comment
#[utoipa::path(
    post,
    params(
        ("comment_id", description = "A numeric ID representing a comment"),
    ),
    path = "/{comment_id}",
    request_body = ContentPayload,
    responses(
        (status = 200, body = CommonResult<String>, content_type = "application/json")
    ),
    tag = CONV_TAG
)]
async fn edit_comment(
    _: LoginUser,
    Path(comment_id): Path<i64>,
    state: State<MonoApiServiceState>,
    Json(payload): Json<ContentPayload>,
) -> Result<Json<CommonResult<String>>, ApiError> {
    state
        .services()
        .conversation()
        .update_comment(comment_id, Some(payload.content))
        .await?;
    Ok(Json(CommonResult::success(None)))
}
