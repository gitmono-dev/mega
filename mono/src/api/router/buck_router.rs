//! Buck upload API router
//!
//! This module provides HTTP routes for the Buck upload batch API.

use api_model::common::CommonResult;
use axum::{
    Json,
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, Request, StatusCode},
};
use ceres::model::buck::*;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::api::{
    MonoApiServiceState, api_common::identity::collaboration_actor, api_doc::BUCK_TAG,
    error::ApiError, oauth::model::LoginUser,
};

pub fn routers() -> OpenApiRouter<MonoApiServiceState> {
    OpenApiRouter::new().nest(
        "/buck",
        OpenApiRouter::new()
            .routes(routes!(create_session))
            .routes(routes!(upload_manifest))
            .routes(routes!(upload_file))
            .routes(routes!(complete_upload)),
    )
}

/// Create upload session
///
/// Creates a new upload session and pre-creates a Draft CL.
#[utoipa::path(
    post,
    path = "/session/start",
    request_body = CreateSessionPayload,
    responses(
        (status = 200, body = CommonResult<SessionResponse>),
        (status = 400, description = "Invalid request parameters"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "Path not found"),
    ),
    tag = BUCK_TAG
)]
async fn create_session(
    user: LoginUser,
    state: State<MonoApiServiceState>,
    Json(payload): Json<CreateSessionPayload>,
) -> Result<Json<CommonResult<SessionResponse>>, ApiError> {
    let actor = collaboration_actor(&user)?;
    let service_resp = state
        .git()
        .create_buck_session(actor, &payload.path)
        .await?;

    let response = SessionResponse {
        cl_link: service_resp.cl_link,
        expires_at: service_resp.expires_at,
        max_file_size: service_resp.max_file_size,
        max_files: service_resp.max_files,
        max_concurrent_uploads: service_resp.max_concurrent_uploads,
    };

    Ok(Json(CommonResult::success(Some(response))))
}

/// Upload file manifest
///
/// Submit file manifest and get list of files that need to be uploaded.
#[utoipa::path(
    post,
    params(
        ("cl_link" = String, Path, description = "CL link (8-character alphanumeric identifier)")
    ),
    path = "/session/{cl_link}/manifest",
    request_body = ManifestPayload,
    responses(
        (status = 200, body = CommonResult<ManifestResponse>),
        (status = 400, description = "Invalid manifest format or empty"),
        (status = 404, description = "Session not found"),
        (status = 409, description = "Invalid session status"),
    ),
    tag = BUCK_TAG
)]
async fn upload_manifest(
    user: LoginUser,
    Path(cl_link): Path<String>,
    state: State<MonoApiServiceState>,
    Json(payload): Json<ManifestPayload>,
) -> Result<Json<CommonResult<ManifestResponse>>, ApiError> {
    let actor = collaboration_actor(&user)?;
    let response = state
        .git()
        .process_buck_manifest(actor, &cl_link, payload)
        .await
        .map_err(ApiError::from)?;

    Ok(Json(CommonResult::success(Some(response))))
}

/// Upload file
///
/// Upload a single file content. Can be called concurrently for different files.
#[utoipa::path(
    post,
    params(
        ("cl_link" = String, Path, description = "CL link (8-character alphanumeric identifier)")
    ),
    path = "/session/{cl_link}/file",
    responses(
        (status = 200, body = CommonResult<FileUploadResponse>),
        (status = 400, description = "Invalid parameters or validation failed"),
        (status = 404, description = "Session not found or file not in manifest"),
        (status = 413, description = "File too large"),
        (status = 415, description = "Invalid Content-Type"),
    ),
    tag = BUCK_TAG
)]
async fn upload_file(
    user: LoginUser,
    Path(cl_link): Path<String>,
    state: State<MonoApiServiceState>,
    headers: HeaderMap,
    req: Request<Body>,
) -> Result<Json<CommonResult<FileUploadResponse>>, ApiError> {
    let actor = collaboration_actor(&user)?;
    use axum::body::to_bytes;
    use percent_encoding::percent_decode_str;

    // Parse file_size from header
    let file_size: u64 = headers
        .get("x-file-size")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| {
            ApiError::bad_request(anyhow::anyhow!("Missing or invalid X-File-Size header"))
        })?;

    // Get max_file_size from BuckService
    let max_size = state.git().buck_max_file_size();
    if file_size > max_size {
        return Err(ApiError::with_status(
            StatusCode::PAYLOAD_TOO_LARGE,
            anyhow::anyhow!("File size {} exceeds limit {}", file_size, max_size),
        ));
    }

    // Acquire permits through BuckService
    let (_global_permit, _large_file_permit) = state
        .git()
        .buck_try_acquire_upload_permits(file_size)
        .map_err(|e| {
            tracing::warn!(
                "Buck upload rate limited: cl_link={}, file_size={}, user={}, error={}",
                cl_link,
                file_size,
                actor,
                e
            );
            ApiError::from(e)
        })?;

    tracing::debug!(
        "Buck upload started: cl_link={}, file_size={}, is_large_file={}, user={}",
        cl_link,
        file_size,
        _large_file_permit.is_some(),
        actor
    );

    // Validate Content-Type (must be present and application/octet-stream)
    match headers.get("content-type") {
        Some(ct) if ct == "application/octet-stream" => {}
        _ => {
            return Err(ApiError::with_status(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                anyhow::anyhow!("Content-Type must be application/octet-stream"),
            ));
        }
    }

    // Parse remaining headers
    let file_path = headers
        .get("x-file-path")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| ApiError::bad_request(anyhow::anyhow!("Missing X-File-Path header")))?;

    let file_path = percent_decode_str(file_path)
        .decode_utf8()
        .map_err(|e| ApiError::bad_request(anyhow::anyhow!("Invalid X-File-Path encoding: {}", e)))?
        .to_string();

    let file_hash = headers
        .get("x-file-hash")
        .and_then(|v| v.to_str().ok())
        .map(String::from);

    // Read body
    let body_bytes = to_bytes(req.into_body(), max_size as usize)
        .await
        .map_err(|e| ApiError::bad_request(anyhow::anyhow!("Failed to read body: {}", e)))?;

    let svc_resp = state
        .git()
        .upload_buck_file(
            actor,
            &cl_link,
            &file_path,
            file_size,
            file_hash.as_deref(),
            body_bytes,
        )
        .await
        .map_err(ApiError::from)?;

    Ok(Json(CommonResult::success(Some(FileUploadResponse {
        file_path: svc_resp.file_path,
        uploaded_size: svc_resp.uploaded_size,
        verified: svc_resp.verified,
    }))))
}

/// Complete upload
///
/// Complete the upload session, create Git commit, and activate CL.
/// Returns immediately - CI build is triggered asynchronously.
///
/// Request body is optional. Commit message is read from session.
#[utoipa::path(
    post,
    params(
        ("cl_link" = String, Path, description = "CL link (8-character alphanumeric identifier)")
    ),
    path = "/session/{cl_link}/complete",
    request_body(
        content = Option<CompletePayload>,
        description = "Optional empty payload. Can be omitted or sent as {}",
        content_type = "application/json"
    ),
    responses(
        (status = 200, body = CommonResult<CompleteResponse>),
        (status = 400, description = "Files not fully uploaded"),
        (status = 404, description = "Session not found"),
        (status = 409, description = "Invalid session status"),
    ),
    tag = BUCK_TAG
)]
async fn complete_upload(
    user: LoginUser,
    Path(cl_link): Path<String>,
    state: State<MonoApiServiceState>,
    payload: Option<Json<CompletePayload>>,
) -> Result<Json<CommonResult<CompleteResponse>>, ApiError> {
    let actor = collaboration_actor(&user)?;
    let payload = payload.map(|p| p.0).unwrap_or(CompletePayload {});
    let response = state
        .git()
        .complete_buck_upload(actor, &cl_link, payload)
        .await
        .map_err(ApiError::from)?;

    Ok(Json(CommonResult::success(Some(response))))
}
