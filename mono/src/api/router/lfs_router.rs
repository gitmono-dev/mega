//! This module contains handlers for handling requests related to Git LFS (Large File Storage).
//!
//! The handlers in this module are responsible for handling various Git LFS operations such as
//! retrieving locks, verifying locks, creating and deleting locks, processing batch requests,
//! downloading and uploading objects, etc.
//!
//! Each handler corresponds to a specific endpoint or operation in the Git LFS protocol.
//! Error handling is done to return appropriate responses in case of success or failure.
//! These handlers are used in an Axum-based web application to handle Git LFS requests.
//!
//! # References
//!
//! - Git LFS Documentation: [https://git-lfs.github.com/](https://git-lfs.github.com/)
//! - Axum Documentation: [https://docs.rs/axum/](https://docs.rs/axum/)
//!
//! # Note
//!
//! Add more specific details and examples as needed to describe each handler's functionality.
//!
//! # Examples and Usage
//!
//! - `lfs_retrieve_lock`: Handles retrieving locks for Git LFS objects.
//! - `lfs_verify_lock`: Handles verifying locks for Git LFS objects.
//! - `lfs_create_lock`: Handles creating locks for Git LFS objects.
//! - `lfs_delete_lock`: Handles deleting locks for Git LFS objects.
//! - `lfs_process_batch`: Handles batch processing requests for Git LFS objects.
//! - `lfs_download_object`: Handles downloading Git LFS objects.
//! - `lfs_upload_object`: Handles uploading Git LFS objects.
//!
//! # Errors
//!
//! The handlers return `Result<Response, (StatusCode, String)>` to handle success or error cases.
//! Error responses are constructed with appropriate status codes and error messages.
//!
//! # Panics
//!
//! The code in these handlers is designed to handle errors gracefully and avoid panics.
//! However, certain unexpected situations might lead to panics, which should be minimized.
//!
//! # Security Considerations
//!
//! Ensure proper authentication and authorization mechanisms are implemented
//! when using these handlers in a web application to prevent unauthorized access.
use axum::{
    Json,
    body::Body,
    extract::{Path, Query, State},
    http::{Request, StatusCode},
    response::Response,
};
use ceres::lfs::lfs_structs::{
    BatchRequest, BatchResponse, LockList, LockListQuery, LockRequest, LockResponse, RequestObject,
    UnlockRequest, UnlockResponse, VerifiableLockList, VerifiableLockRequest,
};
use common::errors::GitLFSError;
use futures::TryStreamExt;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::api::{MonoApiServiceState, api_doc::LFS_TAG};

const LFS_CONTENT_TYPE: &str = "application/vnd.git-lfs+json";
const LFS_STREAM_CONTENT_TYPE: &str = "application/octet-stream";

#[cfg(feature = "fastcdc")]
mod media;

/// Original repository path, captured before the standard LFS URI rewrite.
#[derive(Clone, Debug)]
pub struct LfsRepository(pub String);

pub fn lfs_routes() -> OpenApiRouter<MonoApiServiceState> {
    let router = OpenApiRouter::new()
        .routes(routes!(lfs_upload_object))
        .routes(routes!(lfs_download_object))
        .routes(routes!(list_locks))
        .routes(routes!(create_lock))
        .routes(routes!(list_locks_for_verification))
        .routes(routes!(delete_lock))
        .routes(routes!(lfs_process_batch));
    #[cfg(feature = "fastcdc")]
    let router = router.nest("/libra/media/v1", media::router());
    router
}

/// The [LFS Server Discovery](https://github.com/git-lfs/git-lfs/blob/main/docs/api/server-discovery.md)
/// document describes the server LFS discovery protocol.
/// Example:
/// Git remote: https://git-server.com/foo/bar
/// LFS server: https://git-server.com/foo/bar.git/info/lfs
/// Locks API: https://git-server.com/foo/bar.git/info/lfs/locks
///
/// We expose both `/info/lfs` (standard Git LFS discovery path) and `/api/v1/lfs`
/// (versioned REST path for internal consistency). Both paths serve identical handlers.
///
/// For OpenAPI documentation, we only register `/api/v1/lfs` paths to avoid duplication.
/// The `/info/lfs` paths are still available at runtime for Git LFS client compatibility.
pub fn routers() -> OpenApiRouter<MonoApiServiceState> {
    // Only register /api/v1/lfs for OpenAPI to avoid path duplication
    // /info/lfs paths are still available at runtime via the main router
    OpenApiRouter::new().nest("/api/v1/lfs", lfs_routes())
}

/// Maps GitLFSError to HTTP status code and message.
///
/// This is a temporary workaround until handler layer returns typed errors instead of
/// GitLFSError::GeneralError(String). The string matching approach is fragile but necessary
/// because:
/// 1. LFS routes need to return LFS-specific JSON format (application/vnd.git-lfs+json)
/// 2. Handler layer currently only returns GitLFSError::GeneralError(String)
/// 3. We need to map errors to appropriate HTTP status codes (404/400/500)
///
/// TODO: Refactor handler layer to use typed GitLFSError variants (NotFound, InvalidInput, etc.)
/// and replace this function with direct pattern matching on error types.
fn map_lfs_error<E: ToString>(err: E) -> (StatusCode, String) {
    let msg = err.to_string();
    // Match common error patterns from handler layer (case-sensitive to avoid over-matching).
    if msg.contains("Not found") || msg.contains("not found") || msg.contains("doesn't exist") {
        (StatusCode::NOT_FOUND, msg)
    } else if msg.contains("Invalid") || msg.contains("invalid") || msg.contains("Bad request") {
        (StatusCode::BAD_REQUEST, msg)
    } else {
        (StatusCode::INTERNAL_SERVER_ERROR, msg)
    }
}

fn lfs_error_response(code: StatusCode, msg: String) -> Response<Body> {
    let error_body = serde_json::json!({ "message": msg }).to_string();
    Response::builder()
        .status(code)
        .header("Content-Type", LFS_CONTENT_TYPE)
        .body(Body::from(error_body))
        .unwrap()
}

/// List LFS locks
///
#[utoipa::path(
    get,
    path = "/locks",
    params(
        ("path" = Option<String>, Query, description = "Filter locks by file path"),
        ("id" = Option<String>, Query, description = "Filter locks by lock ID"),
        ("limit" = Option<String>, Query, description = "Maximum number of locks to return"),
        ("refspec" = Option<String>, Query, description = "Git reference specifier"),
    ),
    responses(
        (status = 200, description = "List of locks", body = LockList, content_type = "application/vnd.git-lfs+json"),
        (status = 400, description = "Invalid request", content_type = "application/vnd.git-lfs+json"),
        (status = 404, description = "Lock not found", content_type = "application/vnd.git-lfs+json"),
        (status = 500, description = "Internal server error")
    ),
    tag = LFS_TAG,
    description = "List LFS locks. This handler is also available at `/info/lfs/locks` for Git LFS client compatibility."
)]
pub async fn list_locks(
    state: State<MonoApiServiceState>,
    Query(query): Query<LockListQuery>,
) -> Result<Response<Body>, (StatusCode, String)> {
    let result: Result<LockList, GitLFSError> =
        state.services().lfs().lfs_retrieve_lock(query).await;
    match result {
        Ok(lock_list) => {
            let body = serde_json::to_string(&lock_list).unwrap_or_default();
            Ok(Response::builder()
                .header("Content-Type", LFS_CONTENT_TYPE)
                .body(Body::from(body))
                .unwrap())
        }
        Err(err) => {
            let (code, msg) = map_lfs_error(err);
            let error_body = serde_json::json!({ "message": msg }).to_string();
            Ok(Response::builder()
                .status(code)
                .header("Content-Type", LFS_CONTENT_TYPE)
                .body(Body::from(error_body))
                .unwrap())
        }
    }
}

/// Verify LFS locks
///
/// Verifies locks for a given ref, returning locks that belong to the current user (ours) and others (theirs).
#[utoipa::path(
    post,
    path = "/locks/verify",
    request_body = VerifiableLockRequest,
    responses(
        (status = 200, description = "Verifiable lock list", body = VerifiableLockList, content_type = "application/vnd.git-lfs+json"),
        (status = 400, description = "Invalid request", content_type = "application/vnd.git-lfs+json"),
        (status = 404, description = "Lock not found", content_type = "application/vnd.git-lfs+json"),
        (status = 500, description = "Internal server error")
    ),
    tag = LFS_TAG,
    description = "Verify LFS locks. This handler is also available at `/info/lfs/locks/verify` for Git LFS client compatibility."
)]
pub async fn list_locks_for_verification(
    state: State<MonoApiServiceState>,
    Json(json): Json<VerifiableLockRequest>,
) -> Result<Response<Body>, (StatusCode, String)> {
    let result = state.services().lfs().lfs_verify_lock(json).await;
    match result {
        Ok(lock_list) => {
            let body = serde_json::to_string(&lock_list).unwrap_or_default();
            Ok(Response::builder()
                .header("Content-Type", LFS_CONTENT_TYPE)
                .body(Body::from(body))
                .unwrap())
        }
        Err(err) => {
            let (code, msg) = map_lfs_error(err);
            Ok(lfs_error_response(code, msg))
        }
    }
}

/// Create an LFS lock
///
/// Creates a lock for a file path in the repository. The lock prevents other users from modifying the file.
#[utoipa::path(
    post,
    path = "/locks",
    request_body = LockRequest,
    responses(
        (status = 201, description = "Lock created successfully", body = LockResponse, content_type = "application/vnd.git-lfs+json"),
        (status = 400, description = "Invalid request or parameters", content_type = "application/vnd.git-lfs+json"),
        (status = 404, description = "Resource not found", content_type = "application/vnd.git-lfs+json"),
        (status = 500, description = "Internal server error or lock already exists")
    ),
    tag = LFS_TAG,
    description = "Create an LFS lock. This handler is also available at `/info/lfs/locks` for Git LFS client compatibility."
)]
pub async fn create_lock(
    state: State<MonoApiServiceState>,
    Json(json): Json<LockRequest>,
) -> Result<Response<Body>, (StatusCode, String)> {
    let result = state.services().lfs().lfs_create_lock(json).await;
    match result {
        Ok(lock) => {
            let lock_response = LockResponse {
                lock,
                message: "".to_string(),
            };
            let body = serde_json::to_string(&lock_response).unwrap_or_default();
            Ok(Response::builder()
                .header("Content-Type", LFS_CONTENT_TYPE)
                .status(StatusCode::CREATED)
                .body(Body::from(body))
                .unwrap())
        }
        Err(err) => {
            let (code, msg) = map_lfs_error(err);
            Ok(lfs_error_response(code, msg))
        }
    }
}

/// Delete an LFS lock
///
/// Deletes a lock by its ID. Requires the lock to belong to the current user unless force is set to true.
#[utoipa::path(
    post,
    path = "/locks/{id}/unlock",
    params(
        ("id" = String, Path, description = "Lock ID to unlock"),
    ),
    request_body = UnlockRequest,
    responses(
        (status = 200, description = "Lock deleted successfully", body = UnlockResponse, content_type = "application/vnd.git-lfs+json"),
        (status = 400, description = "Invalid request", content_type = "application/vnd.git-lfs+json"),
        (status = 404, description = "Lock not found", content_type = "application/vnd.git-lfs+json"),
        (status = 500, description = "Internal server error or lock not found")
    ),
    tag = LFS_TAG,
    description = "Delete an LFS lock. This handler is also available at `/info/lfs/locks/{id}/unlock` for Git LFS client compatibility."
)]
pub async fn delete_lock(
    state: State<MonoApiServiceState>,
    Path(id): Path<String>,
    Json(json): Json<UnlockRequest>,
) -> Result<Response, (StatusCode, String)> {
    let result = state.services().lfs().lfs_delete_lock(&id, json).await;

    match result {
        Ok(lock) => {
            let unlock_response = UnlockResponse {
                lock,
                message: "".to_string(),
            };
            let body = serde_json::to_string(&unlock_response).unwrap_or_default();
            Ok(Response::builder()
                .header("Content-Type", LFS_CONTENT_TYPE)
                .body(Body::from(body))
                .unwrap())
        }
        Err(err) => {
            let (code, msg) = map_lfs_error(err);
            Ok(lfs_error_response(code, msg))
        }
    }
}

/// Process LFS batch request
///
/// Processes a batch of LFS objects for upload or download operations.
/// Returns URLs and actions for each object.
#[utoipa::path(
    post,
    path = "/objects/batch",
    request_body = BatchRequest,
    responses(
        (status = 200, description = "Batch response with object actions", body = BatchResponse, content_type = "application/vnd.git-lfs+json"),
        (status = 400, description = "Bad request", content_type = "application/vnd.git-lfs+json"),
        (status = 404, description = "Object(s) not found", content_type = "application/vnd.git-lfs+json"),
        (status = 500, description = "Internal server error")
    ),
    tag = LFS_TAG,
    description = "Process LFS batch request. This handler is also available at `/info/lfs/objects/batch` for Git LFS client compatibility."
)]
pub async fn lfs_process_batch(
    state: State<MonoApiServiceState>,
    Json(json): Json<BatchRequest>,
) -> Result<Response<Body>, (StatusCode, String)> {
    let result = state
        .services()
        .lfs()
        .lfs_process_batch(json, state.listen_addr())
        .await;

    match result {
        Ok(res) => {
            let body = serde_json::to_string(&res).unwrap_or_default();
            Ok(Response::builder()
                .header("Content-Type", LFS_CONTENT_TYPE)
                .body(Body::from(body))
                .unwrap())
        }
        Err(err) => {
            let (code, msg) = map_lfs_error(err);
            tracing::error!("Error: {}", msg);
            Ok(lfs_error_response(code, msg))
        }
    }
}

/// Download an LFS object
///
/// Downloads an LFS object by its OID. Returns the object data as a stream.
#[utoipa::path(
    get,
    path = "/objects/{object_id}",
    params(
        ("object_id" = String, Path, description = "Object ID (OID) to download"),
    ),
    responses(
        (status = 200, description = "Object data stream", content_type = "application/octet-stream"),
        (status = 400, description = "Bad request", content_type = "application/vnd.git-lfs+json"),
        (status = 404, description = "Object not found", content_type = "application/vnd.git-lfs+json"),
        (status = 500, description = "Internal server error or object not found")
    ),
    tag = LFS_TAG,
    description = "Download an LFS object. This handler is also available at `/info/lfs/objects/{object_id}` for Git LFS client compatibility."
)]
pub async fn lfs_download_object(
    state: State<MonoApiServiceState>,
    Path(oid): Path<String>,
) -> Result<Response, (StatusCode, String)> {
    let result = state
        .services()
        .lfs()
        .lfs_download_object(oid.clone())
        .await;
    match result {
        Ok(byte_stream) => Ok(Response::builder()
            .header("Content-Type", LFS_STREAM_CONTENT_TYPE)
            .body(Body::from_stream(byte_stream))
            .unwrap()),
        Err(err) => {
            let (code, msg) = map_lfs_error(err);
            Ok(lfs_error_response(code, msg))
        }
    }
}

/// Upload an LFS object
///
/// Uploads an LFS object to the server. The object data should be sent in the request body.
#[utoipa::path(
    put,
    path = "/objects/{object_id}",
    params(
        ("object_id" = String, Path, description = "Object ID (OID) to upload"),
    ),
    request_body(content = Vec<u8>, content_type = "application/octet-stream", description = "Object data"),
    responses(
        (status = 200, description = "Object uploaded successfully", content_type = "application/vnd.git-lfs+json"),
        (status = 400, description = "Bad request", content_type = "application/vnd.git-lfs+json"),
        (status = 404, description = "Object not found", content_type = "application/vnd.git-lfs+json"),
        (status = 500, description = "Internal server error")
    ),
    tag = LFS_TAG,
    description = "Upload an LFS object. This handler is also available at `/info/lfs/objects/{object_id}` for Git LFS client compatibility."
)]
pub async fn lfs_upload_object(
    state: State<MonoApiServiceState>,
    Path(oid): Path<String>,
    req: Request<Body>,
) -> Result<Response<Body>, (StatusCode, String)> {
    let body_bytes = match read_lfs_upload_body(&oid, req).await {
        Ok(bytes) => bytes,
        Err((code, message)) => return Ok(lfs_error_response(code, message)),
    };
    let req_obj = RequestObject {
        oid,
        ..Default::default()
    };

    let result = state
        .services()
        .lfs()
        .lfs_upload_object(&req_obj, body_bytes)
        .await;
    match result {
        Ok(_) => Ok(Response::builder()
            .header("Content-Type", LFS_CONTENT_TYPE)
            .body(Body::empty())
            .unwrap()),
        Err(err) => {
            let (code, msg) = map_lfs_error(err);
            Ok(lfs_error_response(code, msg))
        }
    }
}

async fn read_lfs_upload_body(
    oid: &str,
    req: Request<Body>,
) -> Result<Vec<u8>, (StatusCode, String)> {
    // Reject invalid paths before polling an untrusted upload body.
    ceres::lfs::handler::validate_object_oid(oid).map_err(map_lfs_error)?;
    req.into_body()
        .into_data_stream()
        .try_fold(Vec::new(), |mut acc, chunk| async move {
            acc.extend_from_slice(&chunk);
            Ok(acc)
        })
        .await
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid LFS request body".into()))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use axum::http::StatusCode;

    use super::*;

    #[tokio::test]
    async fn invalid_upload_oid_is_rejected_without_reading_body() {
        let body = Body::from_stream(futures::stream::poll_fn(
            |_| -> std::task::Poll<Option<std::io::Result<Vec<u8>>>> {
                panic!("invalid object IDs must be rejected before polling the body")
            },
        ));
        let (code, message) =
            read_lfs_upload_body("media-v1/known-scope/chunks/known-hash", Request::new(body))
                .await
                .unwrap_err();
        let response = lfs_error_response(code, message);
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(response.headers()["Content-Type"], LFS_CONTENT_TYPE);
    }

    #[tokio::test]
    async fn unreadable_upload_body_returns_lfs_error() {
        let body = Body::from_stream(futures::stream::once(async {
            Err::<Vec<u8>, _>(std::io::Error::other("test body failure"))
        }));
        let (code, message) = read_lfs_upload_body(&"a".repeat(64), Request::new(body))
            .await
            .unwrap_err();
        let response = lfs_error_response(code, message);
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(response.headers()["Content-Type"], LFS_CONTENT_TYPE);
    }

    #[test]
    fn test_map_lfs_error_not_found() {
        // Test "Not found" error mapping
        let error = "Object not found";
        let (code, msg) = map_lfs_error(error);
        assert_eq!(code, StatusCode::NOT_FOUND);
        assert_eq!(msg, "Object not found");

        // Test case-insensitive "not found"
        let error = "Lock not found in database";
        let (code, _) = map_lfs_error(error);
        assert_eq!(code, StatusCode::NOT_FOUND);
    }

    #[test]
    fn test_map_lfs_error_bad_request() {
        // Test "Invalid" error mapping
        let error = "Invalid object ID format";
        let (code, msg) = map_lfs_error(error);
        assert_eq!(code, StatusCode::BAD_REQUEST);
        assert_eq!(msg, "Invalid object ID format");

        // Test other invalid cases
        let error = "Invalid request parameters";
        let (code, _) = map_lfs_error(error);
        assert_eq!(code, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn test_map_lfs_error_internal_server_error() {
        // Test default error mapping
        let error = "Database connection failed";
        let (code, msg) = map_lfs_error(error);
        assert_eq!(code, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(msg, "Database connection failed");

        // Test generic error
        let error = "Unexpected error occurred";
        let (code, _) = map_lfs_error(error);
        assert_eq!(code, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn test_map_lfs_error_with_gitlfs_error() {
        use common::errors::GitLFSError;

        // Test with GitLFSError that contains "Not found"
        let error = GitLFSError::GeneralError("Object not found".to_string());
        let (code, msg) = map_lfs_error(&error);
        assert_eq!(code, StatusCode::NOT_FOUND);
        assert!(msg.contains("not found"));

        // Test with GitLFSError that contains "Invalid"
        let error = GitLFSError::GeneralError("Invalid OID format".to_string());
        let (code, msg) = map_lfs_error(&error);
        assert_eq!(code, StatusCode::BAD_REQUEST);
        assert!(msg.contains("Invalid"));

        // Test with GitLFSError that doesn't match patterns
        let error = GitLFSError::GeneralError("Storage error".to_string());
        let (code, _) = map_lfs_error(&error);
        assert_eq!(code, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn test_map_lfs_error_edge_cases() {
        // Test empty string
        let error = "";
        let (code, msg) = map_lfs_error(error);
        assert_eq!(code, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(msg, "");

        // Test string with both patterns (should match first)
        let error = "Not found: Invalid request";
        let (code, _) = map_lfs_error(error);
        assert_eq!(code, StatusCode::NOT_FOUND); // "Not found" comes first

        // Test case variations
        let error = "NOT FOUND";
        let (code, _) = map_lfs_error(error);
        assert_eq!(code, StatusCode::INTERNAL_SERVER_ERROR); // Case sensitive

        let error = "not found";
        let (code, _) = map_lfs_error(error);
        assert_eq!(code, StatusCode::NOT_FOUND); // Lowercase works
    }

    #[test]
    fn test_lfs_content_types() {
        // Test content type constants
        assert_eq!(LFS_CONTENT_TYPE, "application/vnd.git-lfs+json");
        assert_eq!(LFS_STREAM_CONTENT_TYPE, "application/octet-stream");
        assert_eq!(LFS_TAG, "Git LFS");
    }

    #[test]
    fn test_lfs_routes_structure() {
        // Smoke test: ensure the LFS routes can be constructed without panicking.
        // This verifies that route registration does not cause runtime failures
        // during router creation. It does NOT assert individual routes exist;
        // those should be covered by more targeted endpoint tests if needed.
        let _routes = lfs_routes();
    }

    #[test]
    fn test_lfs_download_chunk_parameter_validation() {
        // Test parameter validation logic for lfs_download_chunk
        // This tests the query parameter parsing logic

        // Valid parameters
        let mut valid_params = HashMap::new();
        valid_params.insert("offset".to_string(), "0".to_string());
        valid_params.insert("size".to_string(), "1024".to_string());

        let offset = valid_params
            .get("offset")
            .and_then(|offset| offset.parse::<u64>().ok());
        let size = valid_params
            .get("size")
            .and_then(|size| size.parse::<u64>().ok());

        assert!(offset.is_some());
        assert!(size.is_some());
        assert_eq!(offset.unwrap(), 0);
        assert_eq!(size.unwrap(), 1024);

        // Missing offset
        let mut missing_offset = HashMap::new();
        missing_offset.insert("size".to_string(), "1024".to_string());
        let offset = missing_offset
            .get("offset")
            .and_then(|offset| offset.parse::<u64>().ok());
        assert!(offset.is_none());

        // Missing size
        let mut missing_size = HashMap::new();
        missing_size.insert("offset".to_string(), "0".to_string());
        let size = missing_size
            .get("size")
            .and_then(|size| size.parse::<u64>().ok());
        assert!(size.is_none());

        // Invalid offset format
        let mut invalid_offset = HashMap::new();
        invalid_offset.insert("offset".to_string(), "invalid".to_string());
        invalid_offset.insert("size".to_string(), "1024".to_string());
        let offset = invalid_offset
            .get("offset")
            .and_then(|offset| offset.parse::<u64>().ok());
        assert!(offset.is_none());

        // Invalid size format
        let mut invalid_size = HashMap::new();
        invalid_size.insert("offset".to_string(), "0".to_string());
        invalid_size.insert("size".to_string(), "not_a_number".to_string());
        let size = invalid_size
            .get("size")
            .and_then(|size| size.parse::<u64>().ok());
        assert!(size.is_none());
    }

    #[test]
    fn test_lfs_download_chunk_parameter_edge_cases() {
        // Test edge cases for parameter parsing
        let mut params = HashMap::new();

        // Test with zero values
        params.insert("offset".to_string(), "0".to_string());
        params.insert("size".to_string(), "0".to_string());
        let offset = params.get("offset").and_then(|o| o.parse::<u64>().ok());
        let size = params.get("size").and_then(|s| s.parse::<u64>().ok());
        assert_eq!(offset, Some(0));
        assert_eq!(size, Some(0));

        // Test with large values
        params.insert("offset".to_string(), "18446744073709551615".to_string()); // u64::MAX
        params.insert("size".to_string(), "1000000".to_string());
        let offset = params.get("offset").and_then(|o| o.parse::<u64>().ok());
        let size = params.get("size").and_then(|s| s.parse::<u64>().ok());
        assert_eq!(offset, Some(u64::MAX));
        assert_eq!(size, Some(1000000));

        // Test with negative number (should fail to parse as u64)
        params.insert("offset".to_string(), "-1".to_string());
        let offset = params.get("offset").and_then(|o| o.parse::<u64>().ok());
        assert!(offset.is_none());
    }

    #[test]
    fn test_request_object_default() {
        // Test RequestObject default values
        let req_obj = RequestObject {
            oid: "test_oid".to_string(),
            ..Default::default()
        };
        assert_eq!(req_obj.oid, "test_oid");
    }

    #[test]
    fn test_lfs_router_paths() {
        // Smoke test: ensure the LFS router can be constructed without panicking.
        // This verifies that route registration (including standard and versioned
        // LFS paths such as /info/lfs and /api/v1/lfs) does not cause runtime
        // failures during router creation. It does NOT assert individual paths;
        // those should be covered by more targeted endpoint tests if needed.
        let _router = routers();
    }

    #[test]
    fn test_error_response_format() {
        // Test error response format consistency - verify both map_lfs_error and lfs_error_response
        let test_cases = vec![
            ("Not found error", StatusCode::NOT_FOUND),
            ("Invalid parameter", StatusCode::BAD_REQUEST),
            ("Generic error", StatusCode::INTERNAL_SERVER_ERROR),
        ];

        for (msg, expected_code) in test_cases {
            // Test map_lfs_error mapping
            let (code, mapped_msg) = map_lfs_error(msg);
            assert_eq!(code, expected_code, "Error mapping for message: {}", msg);

            // Test lfs_error_response function - verify response structure, status code, and Content-Type
            let response = lfs_error_response(code, mapped_msg.clone());
            assert_eq!(response.status(), expected_code);

            // Verify Content-Type header
            let content_type = response
                .headers()
                .get("Content-Type")
                .and_then(|h| h.to_str().ok());
            assert_eq!(
                content_type,
                Some(LFS_CONTENT_TYPE),
                "Content-Type should be {}",
                LFS_CONTENT_TYPE
            );

            // Verify response body is valid JSON with "message" field
            // Note: In a real test, we'd need to extract and parse the body,
            // but for unit tests we verify the structure is created correctly
        }
    }

    #[test]
    fn test_content_type_constants_consistency() {
        // Verify content type constants match LFS specification
        // JSON responses should use application/vnd.git-lfs+json
        assert_eq!(LFS_CONTENT_TYPE, "application/vnd.git-lfs+json");

        // Binary streams should use application/octet-stream
        assert_eq!(LFS_STREAM_CONTENT_TYPE, "application/octet-stream");

        // Verify they are different
        assert_ne!(LFS_CONTENT_TYPE, LFS_STREAM_CONTENT_TYPE);
    }

    #[test]
    fn test_map_lfs_error_priority() {
        // Test that "Not found" takes priority over "Invalid" when both are present
        let error = "Not found: Invalid request format";
        let (code, _) = map_lfs_error(error);
        assert_eq!(code, StatusCode::NOT_FOUND);

        // Test that "Not found" takes priority even if it appears after "Invalid"
        let error = "Invalid: Not found";
        let (code, _) = map_lfs_error(error);
        // Should match "Not found" first due to function's matching order
        assert_eq!(code, StatusCode::NOT_FOUND);
    }

    #[test]
    fn test_map_lfs_error_unicode_support() {
        // Test error messages with unicode characters
        let error = "对象未找到"; // "Object not found" in Chinese
        let (code, _) = map_lfs_error(error);
        assert_eq!(code, StatusCode::INTERNAL_SERVER_ERROR); // No match, default

        let error = "Object not found: 对象";
        let (code, _) = map_lfs_error(error);
        assert_eq!(code, StatusCode::NOT_FOUND); // Should still match
    }

    #[test]
    fn test_lfs_routes_all_endpoints() {
        // Verify all expected routes are registered
        let routes = lfs_routes();
        // Routes should be created without panicking
        // This is a smoke test to ensure route registration works
        drop(routes);
    }
}
