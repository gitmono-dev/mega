use anyhow::Result;
use axum::{
    Json,
    body::Body,
    extract::{Path, Query, State},
    response::{IntoResponse, Response},
    routing::get,
};
use ceres::{application::api_service::ApiHandler, model::git::TreeQuery};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::api::{
    MonoApiServiceState,
    api_doc::SYSTEM_COMMON,
    error::ApiError,
    notes::note_router,
    router::{
        admin_router, artifacts_router, bot_router, buck_router, build_trigger_router, cl_router,
        code_review_router, commit_router, conv_router, dynamic_sidebar_router, gpg_router,
        group_router, issue_router, label_router, merge_queue_router, orion_runner_router,
        permission_router, preview_router, repo_router, reviewer_router, tag_router, user_router,
        webhook_router,
    },
};

pub fn routers() -> OpenApiRouter<MonoApiServiceState> {
    OpenApiRouter::new()
        .routes(routes!(life_cycle_check))
        .route("/file/blob/{object_id}", get(get_blob_file))
        .route("/file/tree", get(get_tree_file))
        .merge(preview_router::routers())
        .merge(cl_router::routers())
        .merge(reviewer_router::routers())
        .merge(gpg_router::routers())
        .merge(user_router::routers())
        .merge(issue_router::routers())
        .merge(label_router::routers())
        .merge(conv_router::routers())
        .merge(merge_queue_router::routers())
        .merge(note_router::routers())
        .merge(commit_router::routers())
        .merge(tag_router::routers())
        .merge(repo_router::routers())
        .merge(dynamic_sidebar_router::routers())
        .merge(buck_router::routers())
        .merge(admin_router::routers())
        .merge(artifacts_router::routers())
        .merge(group_router::routers())
        .merge(permission_router::routers())
        .merge(code_review_router::routers())
        .merge(build_trigger_router::routers())
        .merge(webhook_router::routers())
        .merge(bot_router::routers())
        .merge(orion_runner_router::routers())
}

/// Health Check
#[utoipa::path(
    get,
    path = "/status",
    responses(
        (status = 200, body = str, content_type = "text/plain")
    ),
    tag = SYSTEM_COMMON
)]
async fn life_cycle_check() -> Result<impl IntoResponse, ApiError> {
    Ok(Json("http ready"))
}

// Blob Objects Download
pub async fn get_blob_file(
    state: State<MonoApiServiceState>,
    Path(oid): Path<String>,
) -> Result<Response, ApiError> {
    let api_handler = state.git();

    let data = api_handler.get_raw_blob_by_hash(&oid).await?;
    let file_name = format!("inline; filename=\"{oid}\"");
    Ok(Response::builder()
        .header("Content-Type", "application/octet-stream")
        .header("Content-Disposition", file_name)
        .body(Body::from(data))
        .unwrap())
}

// Tree Objects Download
pub async fn get_tree_file(
    state: State<MonoApiServiceState>,
    Query(query): Query<TreeQuery>,
) -> Result<Response, ApiError> {
    let data = state
        .api_handler(query.path.as_ref())
        .await?
        .get_binary_tree_by_path(std::path::Path::new(&query.path), query.oid)
        .await?;

    let file_name = format!("inline; filename=\"{}\"", "");
    Ok(Response::builder()
        .header("Content-Type", "application/octet-stream")
        .header("Content-Disposition", file_name)
        .body(Body::from(data))
        .unwrap())
}
