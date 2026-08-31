//! Authenticated, bounded HTTP adapter for the opt-in media protocol.
use axum::{
    Extension, Json,
    body::{Body, to_bytes},
    extract::{FromRef, Path, State},
    http::{Request, StatusCode},
    response::{IntoResponse, Response},
};
use ceres::lfs::media::{
    self, MediaError, MediaScope,
    protocol::{MAX_MANIFEST_SIZE, MediaManifest},
};
use jupiter::{service::lfs_service::LfsService, storage::user_storage::UserStorage};
use utoipa_axum::{router::OpenApiRouter, routes};

use super::{LFS_CONTENT_TYPE, LFS_STREAM_CONTENT_TYPE, LfsRepository};
use crate::api::{MonoApiServiceState, api_doc::LFS_TAG, oauth::AccessTokenUser};

#[derive(Clone)]
pub(super) struct MediaState {
    lfs: LfsService,
    users: UserStorage,
}

#[cfg(test)]
mod tests;

impl FromRef<MonoApiServiceState> for MediaState {
    fn from_ref(state: &MonoApiServiceState) -> Self {
        Self {
            lfs: state.services().lfs().media_service().clone(),
            users: UserStorage::from_ref(state),
        }
    }
}

impl FromRef<MediaState> for UserStorage {
    fn from_ref(state: &MediaState) -> Self {
        state.users.clone()
    }
}

pub(super) fn router<S>() -> OpenApiRouter<S>
where
    S: Clone + Send + Sync + 'static,
    MediaState: FromRef<S>,
    UserStorage: FromRef<S>,
{
    OpenApiRouter::new()
        .routes(routes!(capabilities))
        .routes(routes!(prepare))
        .routes(routes!(upload_chunk))
        .routes(routes!(finalize))
        .routes(routes!(manifest))
        .routes(routes!(download_chunk))
}

fn scope(user: &AccessTokenUser, repo: Option<&LfsRepository>) -> Result<MediaScope, MediaError> {
    let repo = repo.ok_or(MediaError::NotFound)?;
    MediaScope::new(&user.0.campsite_user_id, &repo.0)
}

// Keep errors small until Axum builds the HTTP response at the handler boundary.
#[derive(Debug)]
enum MediaHttpError {
    Media(MediaError),
    Body,
}

impl From<MediaError> for MediaHttpError {
    fn from(error: MediaError) -> Self {
        Self::Media(error)
    }
}

impl IntoResponse for MediaHttpError {
    fn into_response(self) -> Response {
        match self {
            Self::Media(err) => error(err),
            Self::Body => (
                StatusCode::PAYLOAD_TOO_LARGE,
                Json(serde_json::json!({"message":"media body exceeds limit or cannot be read"})),
            )
                .into_response(),
        }
    }
}

fn error(err: MediaError) -> Response {
    let status = match &err {
        MediaError::Invalid(_) => StatusCode::BAD_REQUEST,
        MediaError::NotFound => StatusCode::NOT_FOUND,
        MediaError::Conflict => StatusCode::CONFLICT,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    // Do not return storage keys or backend diagnostics (which include scope IDs).
    if status.is_server_error() {
        tracing::error!("media storage operation failed: {err}");
    }
    let message = if status.is_server_error() {
        "media storage operation failed".to_owned()
    } else {
        err.to_string()
    };
    (
        status,
        [("Content-Type", LFS_CONTENT_TYPE)],
        Json(serde_json::json!({"message": message})),
    )
        .into_response()
}

async fn body(req: Request<Body>, max: usize) -> Result<Vec<u8>, MediaHttpError> {
    to_bytes(req.into_body(), max)
        .await
        .map(|b| b.to_vec())
        .map_err(|_| MediaHttpError::Body)
}

#[utoipa::path(get, path = "/capabilities", responses((status = 200, description = "FastCDC capabilities"), (status = 401, description = "Access token required")), tag = LFS_TAG)]
async fn capabilities(
    user: AccessTokenUser,
    repo: Option<Extension<LfsRepository>>,
) -> Result<Json<serde_json::Value>, MediaHttpError> {
    scope(&user, repo.as_ref().map(|value| &value.0))?;
    Ok(Json(media::protocol::capabilities()))
}

#[utoipa::path(post, path = "/manifests", responses((status = 200, description = "Prepared manifest and missing chunk hashes")), tag = LFS_TAG)]
async fn prepare(
    user: AccessTokenUser,
    repo: Option<Extension<LfsRepository>>,
    State(state): State<MediaState>,
    req: Request<Body>,
) -> Result<Response, MediaHttpError> {
    let scope = scope(&user, repo.as_ref().map(|value| &value.0))?;
    let bytes = body(req, MAX_MANIFEST_SIZE).await?;
    let manifest: MediaManifest = serde_json::from_slice(&bytes)
        .map_err(|_| MediaError::Invalid("malformed manifest JSON".into()))?;
    let response = media::prepare(&state.lfs, &scope, manifest).await?;
    Ok(Json(response).into_response())
}

#[utoipa::path(put, path = "/manifests/{id}/chunks/{hash}", params(("id" = String, Path), ("hash" = String, Path)), responses((status = 204, description = "Chunk verified and stored")), tag = LFS_TAG)]
async fn upload_chunk(
    user: AccessTokenUser,
    repo: Option<Extension<LfsRepository>>,
    State(state): State<MediaState>,
    Path((id, hash)): Path<(String, String)>,
    req: Request<Body>,
) -> Result<StatusCode, MediaHttpError> {
    let scope = scope(&user, repo.as_ref().map(|value| &value.0))?;
    let bytes = body(req, media::chunker::MAX_SIZE).await?;
    media::upload_chunk(&state.lfs, &scope, &id, &hash, bytes).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(post, path = "/manifests/{id}/finalize", params(("id" = String, Path)), responses((status = 204, description = "Verified manifest and standard LFS fallback published")), tag = LFS_TAG)]
async fn finalize(
    user: AccessTokenUser,
    repo: Option<Extension<LfsRepository>>,
    State(state): State<MediaState>,
    Path(id): Path<String>,
) -> Result<StatusCode, MediaHttpError> {
    let scope = scope(&user, repo.as_ref().map(|value| &value.0))?;
    media::finalize(&state.lfs, &scope, &id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(get, path = "/manifests/by-media/{oid}", params(("oid" = String, Path)), responses((status = 200, description = "Finalized manifest"), (status = 404, description = "No readable manifest")), tag = LFS_TAG)]
async fn manifest(
    user: AccessTokenUser,
    repo: Option<Extension<LfsRepository>>,
    State(state): State<MediaState>,
    Path(oid): Path<String>,
) -> Result<Response, MediaHttpError> {
    let scope = scope(&user, repo.as_ref().map(|value| &value.0))?;
    let response = media::get_manifest(&state.lfs, &scope, &oid).await?;
    Ok(Json(response).into_response())
}

#[utoipa::path(get, path = "/manifests/by-media/{oid}/chunks/{hash}", params(("oid" = String, Path), ("hash" = String, Path)), responses((status = 200, description = "Verified chunk bytes")), tag = LFS_TAG)]
async fn download_chunk(
    user: AccessTokenUser,
    repo: Option<Extension<LfsRepository>>,
    State(state): State<MediaState>,
    Path((oid, hash)): Path<(String, String)>,
) -> Result<Response, MediaHttpError> {
    let scope = scope(&user, repo.as_ref().map(|value| &value.0))?;
    let bytes = media::download_chunk(&state.lfs, &scope, &oid, &hash).await?;
    Ok(([("Content-Type", LFS_STREAM_CONTENT_TYPE)], bytes).into_response())
}
