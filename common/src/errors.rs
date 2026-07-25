use std::convert::Infallible;

use anyhow::Result;
use cedar_policy::ParseErrors;
use config::ConfigError;
use git_internal::errors::GitError;
use thiserror::Error;

pub type MegaResult = Result<(), MegaError>;

#[derive(Error, Debug)]
pub enum MegaError {
    #[error("config error: {0}")]
    Config(#[from] ConfigError),

    // --- Redis ---
    #[error("Redis error: {0}")]
    Redis(#[from] redis::RedisError),

    #[error("serialization error: {0}")]
    EncodeError(#[from] rkyv::rancor::Error),

    // --- Serialization / parsing ---
    #[error("JSON serialization error: {0}")]
    SerdeJson(#[from] serde_json::Error),

    // --- IO ---
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    // --- Database ---
    #[error("Database error: {0}")]
    Db(#[from] sea_orm::DbErr),

    // --- PGP ---
    #[error("PGP error: {0}")]
    Pgp(#[from] Box<pgp::errors::Error>),

    // --- Clap ---
    #[error("Clap error: {0}")]
    Clap(#[from] clap::Error),

    // --- Anyhow ---
    #[error("Generic error: {0}")]
    Anyhow(#[from] anyhow::Error),

    // --- GitError ---
    #[error("Git error: {0}")]
    Git(#[from] GitError),

    // --- BuckError ---
    #[error("Buck API error: {0}")]
    Buck(#[from] BuckError),

    #[error("Not Found error: {0}")]
    NotFound(String),

    #[error("Bad request: {0}")]
    BadRequest(String),

    #[error("Conflict: {0}")]
    Conflict(String),

    #[error("Forbidden: {0}")]
    Forbidden(String),

    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    #[error("Service unavailable: {0}")]
    Unavailable(String),

    #[error("ObjStorage error: {0}")]
    ObjStorage(String),

    /// Object not found in underlying object storage (S3/GCS/local).
    /// Typically corresponds to a 404/NoSuchKey-style error.
    #[error("ObjStorage not found: {0}")]
    ObjStorageNotFound(String),

    ///Object not found in underlying object storage (S3/GCS/local). but exists in the repository.
    #[error("ObjStorage inconsistent: {0}")]
    ObjStorageInconsistent(String),

    /// Monorepo root `mega_refs` row changed before attach finished; caller should re-read head and retry.
    #[error("Monorepo root ref changed concurrently (attach should retry)")]
    StaleMonorepoRootRef,

    // --- Other ---
    #[error("Other error: {0}")]
    Other(String),
}

impl MegaError {
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self::BadRequest(msg.into())
    }

    pub fn conflict(msg: impl Into<String>) -> Self {
        Self::Conflict(msg.into())
    }

    pub fn forbidden(msg: impl Into<String>) -> Self {
        Self::Forbidden(msg.into())
    }

    pub fn unauthorized(msg: impl Into<String>) -> Self {
        Self::Unauthorized(msg.into())
    }

    pub fn unavailable(msg: impl Into<String>) -> Self {
        Self::Unavailable(msg.into())
    }

    /// Parse legacy `[code:xxx] message` strings into typed variants.
    pub fn from_legacy_message(msg: impl Into<String>) -> Self {
        let msg = msg.into();
        if let Some((code, clean)) = parse_legacy_http_code(&msg) {
            return match code {
                400 => Self::BadRequest(clean.to_string()),
                401 => Self::Unauthorized(clean.to_string()),
                403 => Self::Forbidden(clean.to_string()),
                404 => Self::NotFound(clean.to_string()),
                409 => Self::Conflict(clean.to_string()),
                413 => Self::BadRequest(clean.to_string()),
                416 => Self::BadRequest(clean.to_string()),
                503 => Self::Unavailable(clean.to_string()),
                _ => Self::Other(msg),
            };
        }
        Self::Other(msg)
    }
}

impl From<Infallible> for MegaError {
    fn from(err: Infallible) -> MegaError {
        match err {}
    }
}

impl From<ParseErrors> for MegaError {
    fn from(err: ParseErrors) -> MegaError {
        MegaError::Other(err.to_string())
    }
}

/// Parse `[code:xxx]` anywhere in a message. Returns (status_code, clean_message).
pub fn parse_legacy_http_code(err_str: &str) -> Option<(u16, &str)> {
    let start = err_str.find("[code:")?;
    let code_start = start + 6;
    let remaining = &err_str[start..];
    let code_end_relative = remaining.find(']')?;
    if code_end_relative <= 6 {
        return None;
    }
    let code_end = start + code_end_relative;
    let code = &err_str[code_start..code_end];
    if code.is_empty() || !code.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let status: u16 = code.parse().ok()?;
    let msg_start = code_end + 1;
    let msg = err_str.get(msg_start..).unwrap_or("").trim_start();
    Some((status, msg))
}

pub fn buck_error_http_status(err: &BuckError) -> u16 {
    match err {
        BuckError::SessionNotFound(_) | BuckError::FileNotInManifest(_) => 404,
        BuckError::SessionExpired => 410,
        BuckError::RateLimitExceeded => 429,
        BuckError::FileSizeExceedsLimit(_, _) => 413,
        BuckError::FileAlreadyUploaded(_) => 409,
        BuckError::Forbidden(_) => 403,
        BuckError::HashMismatch { .. }
        | BuckError::ValidationError(_)
        | BuckError::InvalidSessionStatus { .. }
        | BuckError::FilesNotFullyUploaded { .. } => 400,
    }
}

pub fn git_error_http_status(err: &GitError) -> u16 {
    match err {
        GitError::ObjectNotFound(_) | GitError::RepoNotFound | GitError::NotFoundHashValue(_) => {
            404
        }
        GitError::UnAuthorized(_) => 401,
        GitError::InvalidObjectType(_)
        | GitError::InvalidBlobObject(_)
        | GitError::InvalidTreeObject
        | GitError::InvalidTreeItem(_)
        | GitError::EmptyTreeItems(_)
        | GitError::InvalidSignatureType(_)
        | GitError::InvalidCommitObject
        | GitError::InvalidCommit(_)
        | GitError::InvalidTagObject(_)
        | GitError::InvalidNoteObject(_)
        | GitError::InvalidPathError(_)
        | GitError::ConversionError(_) => 400,
        GitError::CustomError(msg) => parse_legacy_http_code(msg)
            .map(|(code, _)| code)
            .unwrap_or_else(|| git_custom_error_http_status(msg)),
        _ => 500,
    }
}

fn git_custom_error_http_status(msg: &str) -> u16 {
    let lower = msg.to_ascii_lowercase();
    if lower.contains("not found") || lower.contains("doesn't exist") {
        404
    } else if lower.contains("duplicate")
        || lower.contains("invalid")
        || lower.contains("required")
        || lower.contains("conflict")
    {
        400
    } else if lower.contains("forbidden") || lower.contains("denied") {
        403
    } else {
        500
    }
}

/// Suggested HTTP status code for a [`MegaError`].
pub fn mega_error_http_status(err: &MegaError) -> u16 {
    match err {
        MegaError::NotFound(_) | MegaError::ObjStorageNotFound(_) => 404,
        MegaError::BadRequest(_) => 400,
        MegaError::Conflict(_)
        | MegaError::StaleMonorepoRootRef
        | MegaError::ObjStorageInconsistent(_) => 409,
        MegaError::Forbidden(_) => 403,
        MegaError::Unauthorized(_) => 401,
        MegaError::Unavailable(_) => 503,
        MegaError::Buck(buck_err) => buck_error_http_status(buck_err),
        MegaError::Git(git_err) => git_error_http_status(git_err),
        MegaError::Other(msg) => parse_legacy_http_code(msg)
            .map(|(code, _)| code)
            .unwrap_or(500),
        MegaError::Db(_)
        | MegaError::Redis(_)
        | MegaError::Io(_)
        | MegaError::Config(_)
        | MegaError::EncodeError(_)
        | MegaError::SerdeJson(_)
        | MegaError::Pgp(_)
        | MegaError::Clap(_)
        | MegaError::Anyhow(_)
        | MegaError::ObjStorage(_) => 500,
    }
}

/// Whether the error message is safe to expose to API clients.
pub fn mega_error_is_client_safe(err: &MegaError) -> bool {
    let status = mega_error_http_status(err);
    status < 500 || status == 503
}

impl From<MegaError> for GitError {
    fn from(val: MegaError) -> Self {
        match val {
            MegaError::NotFound(msg) => GitError::CustomError(msg),
            MegaError::BadRequest(msg) => GitError::CustomError(msg),
            MegaError::Conflict(msg) => GitError::CustomError(msg),
            MegaError::Forbidden(msg) => GitError::CustomError(msg),
            MegaError::Unauthorized(msg) => GitError::CustomError(msg),
            MegaError::Unavailable(msg) => GitError::CustomError(msg),
            MegaError::ObjStorageNotFound(msg) => {
                GitError::CustomError(format!("ObjStorage not found: {msg}"))
            }
            other => GitError::CustomError(other.to_string()),
        }
    }
}

#[derive(Error, Debug)]
pub enum GitLFSError {
    #[error("Something went wrong in Git LFS: {0}")]
    GeneralError(String),
}

/// Buck upload API errors
#[derive(Debug, Error)]
pub enum BuckError {
    #[error("Session not found: {0}")]
    SessionNotFound(String),

    #[error("Session expired")]
    SessionExpired,

    #[error("File not in manifest: {0}")]
    FileNotInManifest(String),

    #[error("Rate limit exceeded")]
    RateLimitExceeded,

    #[error("File size exceeds limit: {0} > {1}")]
    FileSizeExceedsLimit(u64, u64),

    #[error("File already uploaded: {0}")]
    FileAlreadyUploaded(String),

    #[error("Hash mismatch: expected {expected}, got {actual}")]
    HashMismatch { expected: String, actual: String },

    #[error("Validation error: {0}")]
    ValidationError(String),

    #[error("Forbidden: {0}")]
    Forbidden(String),

    #[error("Invalid session status: expected {expected:?}, got {actual:?}")]
    InvalidSessionStatus { expected: String, actual: String },

    #[error("Files not fully uploaded: {missing_count} files remaining")]
    FilesNotFullyUploaded { missing_count: u32 },
}

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("{0}")]
    IO(#[from] std::io::Error),
    #[error("Authentication failed: {0}")]
    Deny(String),
    #[error("Repository not found: {0}")]
    NotFound(String),
    #[error("PackFile too large: {0}")]
    TooLarge(String),
    #[error("Invalid Input: {0}")]
    InvalidInput(String),
    #[error("HTTP Push Has Been Disabled")]
    Disabled,
}

pub fn protocol_error_http_status(err: &ProtocolError) -> u16 {
    match err {
        ProtocolError::NotFound(_) => 404,
        ProtocolError::InvalidInput(_) => 400,
        ProtocolError::Deny(_) => 401,
        ProtocolError::TooLarge(_) => 413,
        ProtocolError::Disabled => 403,
        ProtocolError::IO(_) => 500,
    }
}

/// Whether the protocol error message is safe to expose to clients.
pub fn protocol_error_is_client_safe(err: &ProtocolError) -> bool {
    protocol_error_http_status(err) < 500
}

/// Explicit transport-boundary mapping from domain errors to protocol errors.
pub fn mega_to_protocol_error(err: MegaError) -> ProtocolError {
    match err {
        MegaError::NotFound(msg) => ProtocolError::NotFound(msg),
        MegaError::ObjStorageNotFound(msg) => {
            // Missing blob/object content: return a typed client error instead of
            // panicking in upload-pack (which surfaces to git as HTTP 502).
            ProtocolError::NotFound(format!("git object missing from object storage: {msg}"))
        }
        MegaError::ObjStorageInconsistent(msg) => {
            ProtocolError::InvalidInput(format!("git object storage inconsistent: {msg}"))
        }
        MegaError::BadRequest(msg) => ProtocolError::InvalidInput(msg),
        MegaError::Unauthorized(msg) => ProtocolError::Deny(msg),
        MegaError::Forbidden(msg) => ProtocolError::InvalidInput(msg),
        MegaError::Unavailable(msg) => ProtocolError::InvalidInput(msg),
        MegaError::Conflict(msg) => ProtocolError::InvalidInput(msg),
        MegaError::Git(e) => git_to_protocol_error(e),
        MegaError::Io(e) => ProtocolError::IO(e),
        other => ProtocolError::InvalidInput(other.to_string()),
    }
}

/// Map [`GitError`] into a protocol-layer error for upload/receive-pack handlers.
pub fn git_to_protocol_error(err: GitError) -> ProtocolError {
    let msg = err.to_string();
    match git_error_http_status(&err) {
        404 => ProtocolError::NotFound(msg),
        401 => ProtocolError::Deny(msg),
        400 | 403 | 409 | 413 => ProtocolError::InvalidInput(msg),
        _ => ProtocolError::IO(std::io::Error::other(msg)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_legacy_http_code_extracts_status_and_message() {
        let (code, msg) = parse_legacy_http_code("[code:404] CL not found: abc").unwrap();
        assert_eq!(code, 404);
        assert_eq!(msg, "CL not found: abc");
    }

    #[test]
    fn mega_error_from_legacy_message_maps_typed_variants() {
        let err = MegaError::from_legacy_message("[code:503] Build system is not enabled");
        assert!(matches!(err, MegaError::Unavailable(_)));
        assert_eq!(mega_error_http_status(&err), 503);
    }

    #[test]
    fn mega_error_http_status_maps_not_found() {
        assert_eq!(
            mega_error_http_status(&MegaError::NotFound("CL not found".into())),
            404
        );
    }

    #[test]
    fn git_error_http_status_maps_custom_not_found() {
        assert_eq!(
            git_error_http_status(&GitError::CustomError("File not found".into())),
            404
        );
    }

    #[test]
    fn protocol_error_http_status_maps_not_found() {
        assert_eq!(
            protocol_error_http_status(&ProtocolError::NotFound("repo missing".into())),
            404
        );
    }

    #[test]
    fn protocol_error_http_status_maps_disabled() {
        assert_eq!(protocol_error_http_status(&ProtocolError::Disabled), 403);
    }

    #[test]
    fn mega_to_protocol_error_preserves_not_found() {
        let err = mega_to_protocol_error(MegaError::NotFound("repo".into()));
        assert!(matches!(err, ProtocolError::NotFound(_)));
    }

    #[test]
    fn mega_to_protocol_error_maps_objstorage_not_found() {
        let err = mega_to_protocol_error(MegaError::ObjStorageNotFound("missing key".into()));
        assert!(matches!(err, ProtocolError::NotFound(_)));
        assert_eq!(protocol_error_http_status(&err), 404);
    }

    #[test]
    fn git_to_protocol_error_maps_objstorage_custom_error() {
        let err = git_to_protocol_error(GitError::CustomError(
            "ObjStorage not found: Object at location git/ab/cd/ef/123 not found".into(),
        ));
        assert!(matches!(err, ProtocolError::NotFound(_)));
        assert_eq!(protocol_error_http_status(&err), 404);
    }
}
