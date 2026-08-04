use std::path::PathBuf;

use api_model::common::CommonResult;
use axum::{Json, extract::State};
use ceres::{
    application::api_service::mono::MonoServiceLogic, model::change_list::CloneRepoPayload,
};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::api::{
    MonoApiServiceState, api_common::identity::collaboration_actor, api_doc::REPO_TAG,
    error::ApiError, oauth::model::LoginUser,
};

pub fn routers() -> OpenApiRouter<MonoApiServiceState> {
    OpenApiRouter::new().nest(
        "/repo",
        OpenApiRouter::new().routes(routes!(clone_third_party_repo)),
    )
}

// Clone a Github Repo
#[utoipa::path(
    post,
    path = "/clone",
    request_body (
        content = CloneRepoPayload,
    ),
    responses(
        (status = 200, body = CommonResult<String>, content_type = "application/json")
    ),
    tag = REPO_TAG
)]
async fn clone_third_party_repo(
    user: LoginUser,
    state: State<MonoApiServiceState>,
    Json(payload): Json<CloneRepoPayload>,
) -> Result<Json<CommonResult<String>>, ApiError> {
    let actor = collaboration_actor(&user)?;
    let path = MonoServiceLogic::validate_github_sync_path(&payload.path)?;
    let path = PathBuf::from(path);
    state
        .services()
        .git()
        .sync_third_party_repo(
            state.services().transport_runtime(),
            &payload.owner,
            &payload.repo,
            path,
            actor,
        )
        .await?;

    Ok(Json(CommonResult::success(None)))
}
