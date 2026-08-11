use std::{collections::HashMap, path::PathBuf};

use anyhow::Result;
use api_model::{common::CommonResult, git::commit::LatestCommitInfo};
use axum::{
    Json,
    extract::{Query, State},
};
use ceres::model::{
    blame::{BlameQuery, BlameRequest, BlameResult},
    change_list::DiffItemSchema,
    git::{
        BlobContentQuery, CodePreviewQuery, CreateEntryInfo, CreateEntryResult, DiffPreviewPayload,
        EditFilePayload, EditFileResult, FileTreeItem, TreeCommitItem, TreeHashItem, TreeResponse,
    },
};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::api::{
    MonoApiServiceState, api_common::identity::collaboration_actor, api_doc::CODE_PREVIEW,
    error::ApiError, oauth::model::LoginUser,
};

async fn upsert_commit_binding(
    state: &MonoApiServiceState,
    commit_id: &str,
    author_username: Option<&str>,
) -> Result<(), ApiError> {
    let final_username = author_username.and_then(|u| {
        let t = u.trim();
        if t.is_empty() || t.eq_ignore_ascii_case("anonymous") {
            None
        } else {
            Some(t.to_string())
        }
    });
    state
        .git()
        .upsert_commit_binding(commit_id, final_username.clone(), final_username.is_none())
        .await?;
    Ok(())
}

/// Web edits persist the session campsite public id as CL author.
fn resolve_edit_author(
    user: &LoginUser,
    _client_author: Option<String>,
) -> Result<String, ApiError> {
    Ok(collaboration_actor(user)?.to_string())
}

pub fn routers() -> OpenApiRouter<MonoApiServiceState> {
    OpenApiRouter::new()
        .routes(routes!(create_entry))
        .routes(routes!(get_latest_commit))
        .routes(routes!(get_tree_commit_info))
        .routes(routes!(get_file_blame))
        .routes(routes!(get_tree_content_hash))
        .routes(routes!(get_tree_dir_hash))
        .routes(routes!(path_can_be_cloned))
        .routes(routes!(get_tree_info))
        .routes(routes!(get_blob_string))
        .routes(routes!(preview_diff))
        .routes(routes!(save_edit))
}

/// Get blob file as string
#[utoipa::path(
    get,
    params(
        BlobContentQuery
    ),
    path = "/blob",
    responses(
        (status = 200, body = CommonResult<String>, content_type = "application/json")
    ),
    tag = CODE_PREVIEW
)]
async fn get_blob_string(
    Query(query): Query<BlobContentQuery>,
    state: State<MonoApiServiceState>,
) -> Result<Json<CommonResult<String>>, ApiError> {
    let data = state
        .api_handler(query.path.as_ref())
        .await?
        .get_blob_as_string(query.path.into(), Some(query.refs.as_str()))
        .await?;
    Ok(Json(CommonResult::success(data)))
}

/// Create file or folder in web UI
#[utoipa::path(
    post,
    path = "/create-entry",
    request_body = CreateEntryInfo,
    responses(
        (status = 200, body = CommonResult<CreateEntryResult>, content_type = "application/json")
    ),
    tag = CODE_PREVIEW
)]
async fn create_entry(
    user: LoginUser,
    state: State<MonoApiServiceState>,
    Json(mut json): Json<CreateEntryInfo>,
) -> Result<Json<CommonResult<CreateEntryResult>>, ApiError> {
    json.author_username = Some(resolve_edit_author(&user, json.author_username.take())?);
    let handler = state.api_handler(json.path.as_ref()).await?;
    let result = handler.create_monorepo_entry(json.clone()).await?;

    upsert_commit_binding(&state, &result.commit_id, json.author_username.as_deref()).await?;
    Ok(Json(CommonResult::success(Some(result))))
}

/// Get latest commit by path
#[utoipa::path(
    get,
    path = "/latest-commit",
    params(
        CodePreviewQuery
    ),
    responses(
        (status = 200, body = LatestCommitInfo, content_type = "application/json")
    ),
    tag = CODE_PREVIEW
)]
async fn get_latest_commit(
    Query(query): Query<CodePreviewQuery>,
    state: State<MonoApiServiceState>,
) -> Result<Json<LatestCommitInfo>, ApiError> {
    let query_path: std::path::PathBuf = query.path.into();

    // Pass refs as None if empty, ensuring consistent behavior for both tag and commit SHA
    let refs_opt = if query.refs.is_empty() {
        None
    } else {
        Some(query.refs.as_str())
    };

    tracing::debug!(
        "get_latest_commit with path: {:?}, refs: {:?}",
        query_path,
        refs_opt
    );

    // api_handler automatically determines whether to use monorepo or import handler
    let api_handler = state.api_handler(&query_path).await?;

    let res = api_handler.get_latest_commit(query_path, refs_opt).await?;

    Ok(Json(res))
}

/// Get tree brief info
#[utoipa::path(
    get,
    path = "/tree",
    params(
        CodePreviewQuery
    ),
    responses(
        (status = 200, body = CommonResult<TreeResponse>, content_type = "application/json")
    ),
    tag = CODE_PREVIEW
)]
async fn get_tree_info(
    Query(query): Query<CodePreviewQuery>,
    state: State<MonoApiServiceState>,
) -> Result<Json<CommonResult<TreeResponse>>, ApiError> {
    let mut parts = Vec::new();

    let normalized_path = PathBuf::from(query.path.clone());
    let mut segments = normalized_path.components().peekable();
    let mut current = String::new();

    while let Some(segment) = segments.next() {
        let part = segment.as_os_str().to_string_lossy().to_string();
        if segments.peek().is_some() {
            if current != "/" && part != "/" {
                current.push('/');
            }
            current.push_str(&part);
            parts.push(current.clone());
        }
    }

    let mut file_tree = HashMap::new();

    for part in parts {
        let path = part.as_ref();
        let handler = state.api_handler(path).await?;
        let tree_items = handler
            .get_tree_info(path, Some(query.refs.as_str()))
            .await?;
        file_tree.insert(
            part,
            FileTreeItem {
                total_count: tree_items.len(),
                tree_items,
            },
        );
    }

    let tree_items = state
        .api_handler(query.path.as_ref())
        .await?
        .get_tree_info(query.path.as_ref(), Some(query.refs.as_str()))
        .await?;
    Ok(Json(CommonResult::success(Some(TreeResponse {
        file_tree,
        tree_items,
    }))))
}

/// List matching trees with commit msg by query
#[utoipa::path(
    get,
    path = "/tree/commit-info",
    params(
        CodePreviewQuery
    ),
    responses(
        (status = 200, body = CommonResult<Vec<TreeCommitItem>>, content_type = "application/json")
    ),
    tag = CODE_PREVIEW
)]
async fn get_tree_commit_info(
    Query(query): Query<CodePreviewQuery>,
    state: State<MonoApiServiceState>,
) -> Result<Json<CommonResult<Vec<TreeCommitItem>>>, ApiError> {
    let data = state
        .api_handler(query.path.as_ref())
        .await?
        .get_tree_commit_info(query.path.into(), Some(query.refs.as_str()))
        .await?;
    Ok(Json(CommonResult::success(Some(data))))
}

/// Get tree content hash,the dir's hash as same as old,file's hash is the content hash
#[utoipa::path(
    get,
    path = "/tree/content-hash",
    params(
        CodePreviewQuery
    ),
    responses(
        (status = 200, body = CommonResult<Vec<TreeHashItem>>, content_type = "application/json")
    ),
    tag = CODE_PREVIEW
)]
async fn get_tree_content_hash(
    Query(query): Query<CodePreviewQuery>,
    state: State<MonoApiServiceState>,
) -> Result<Json<CommonResult<Vec<TreeHashItem>>>, ApiError> {
    let data = state
        .api_handler(query.path.as_ref())
        .await?
        .get_tree_content_hash(query.path.into(), Some(query.refs.as_str()))
        .await?;
    Ok(Json(CommonResult::success(Some(data))))
}

/// return the dir's hash
#[utoipa::path(
    get,
    path = "/tree/dir-hash",
    params(
        CodePreviewQuery
    ),
    responses(
        (status = 200, body = CommonResult<Vec<TreeHashItem>>, content_type = "application/json")
    ),
    tag = CODE_PREVIEW
)]
async fn get_tree_dir_hash(
    Query(query): Query<CodePreviewQuery>,
    state: State<MonoApiServiceState>,
) -> Result<Json<CommonResult<Vec<TreeHashItem>>>, ApiError> {
    let path = std::path::Path::new(&query.path);
    let parent_path = path
        .parent()
        .and_then(|p| p.to_str())
        .unwrap_or("")
        .to_string();
    let target_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

    let data = state
        .api_handler(parent_path.as_ref())
        .await?
        .get_tree_dir_hash(parent_path.into(), target_name, Some(query.refs.as_str()))
        .await?;

    Ok(Json(CommonResult::success(Some(data))))
}

/// Check if a path can be cloned
#[utoipa::path(
    get,
    path = "/tree/path-can-clone",
    params(
        BlobContentQuery
    ),
    responses(
        (status = 200, body = CommonResult<bool>, content_type = "application/json")
    ),
    tag = CODE_PREVIEW
)]
async fn path_can_be_cloned(
    Query(query): Query<BlobContentQuery>,
    state: State<MonoApiServiceState>,
) -> Result<Json<CommonResult<bool>>, ApiError> {
    let path: PathBuf = query.path.clone().into();
    let import_dir = state.git().import_dir();
    let path_str = path.to_str().unwrap();
    let res = if path.starts_with(&import_dir) {
        match state.git().find_git_repo_like_path(path_str).await? {
            Some(model) => model.repo_path == path_str,
            None => false,
        }
    } else {
        // any path under monorepo can be cloned
        true
    };
    Ok(Json(CommonResult::success(Some(res))))
}

/// Get blame information for a file
#[utoipa::path(
    get,
    path = "/blame",
    params(
        BlameRequest
    ),
    responses(
        (status = 200, body = CommonResult<BlameResult>, content_type = "application/json")
    ),
    tag = CODE_PREVIEW
)]
async fn get_file_blame(
    Query(params): Query<BlameRequest>,
    State(state): State<MonoApiServiceState>,
) -> Result<Json<CommonResult<BlameResult>>, ApiError> {
    let ref_name = if params.refs.is_empty() {
        None
    } else {
        Some(params.refs.as_str())
    };

    let query = BlameQuery::from(&params);
    let result = state
        .api_handler(params.path.as_ref())
        .await?
        .get_file_blame(&params.path, ref_name, query)
        .await?;
    Ok(Json(CommonResult::success(Some(result))))
}

/// Preview unified diff for a single file before saving
#[utoipa::path(
    post,
    path = "/edit/diff-preview",
    request_body = DiffPreviewPayload,
    responses(
        (status = 200, body = CommonResult<DiffItemSchema>, content_type = "application/json")
    ),
    tag = CODE_PREVIEW
)]
async fn preview_diff(
    state: State<MonoApiServiceState>,
    Json(payload): Json<DiffPreviewPayload>,
) -> Result<Json<CommonResult<DiffItemSchema>>, ApiError> {
    let handler = state.api_handler(payload.path.as_ref()).await?;
    let item = handler.preview_file_diff(payload).await?.map(|x| x.into());
    Ok(Json(CommonResult::success(item)))
}

/// Save edit and create a commit
#[utoipa::path(
    post,
    path = "/edit/save",
    request_body = EditFilePayload,
    responses(
        (status = 200, body = CommonResult<EditFileResult>, content_type = "application/json")
    ),
    tag = CODE_PREVIEW
)]
async fn save_edit(
    user: LoginUser,
    state: State<MonoApiServiceState>,
    Json(mut payload): Json<EditFilePayload>,
) -> Result<Json<CommonResult<EditFileResult>>, ApiError> {
    payload.author_username = Some(resolve_edit_author(&user, payload.author_username.take())?);
    let handler = state.api_handler(payload.path.as_ref()).await?;
    let res = handler.save_file_edit(payload.clone()).await?;

    upsert_commit_binding(&state, &res.commit_id, payload.author_username.as_deref()).await?;

    Ok(Json(CommonResult::success(Some(res))))
}
