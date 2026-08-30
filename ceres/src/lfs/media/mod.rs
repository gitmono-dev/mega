//! Opt-in FastCDC transport. Chunks are private to an authenticated user and
//! repository; only a finalized manifest permits reads. Full LFS objects remain
//! the interoperable source of truth. See docs/lfs-api.md for deployment limits.
use std::{
    collections::HashSet,
    sync::LazyLock,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use bytes::Bytes;
use futures::StreamExt;
use io_orbit::object_storage::{ObjectByteStream, ObjectKey, ObjectMeta, ObjectNamespace};
use jupiter::{service::lfs_service::LfsService, utils::into_obj_stream::IntoObjectStream};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncSeekExt, AsyncWriteExt},
    sync::Semaphore,
};

pub mod chunker;
pub mod protocol;
use protocol::{ManifestResponse, MediaManifest, PrepareResponse, valid_hash};

#[cfg(test)]
mod tests;

const PENDING_TTL: Duration = Duration::from_secs(24 * 3600);
static FINALIZERS: LazyLock<Semaphore> = LazyLock::new(|| Semaphore::new(2));

#[derive(Debug, thiserror::Error)]
pub enum MediaError {
    #[error("Invalid media request: {0}")]
    Invalid(String),
    #[error("Media object not found")]
    NotFound,
    #[error("Media manifest conflicts with the finalized object")]
    Conflict,
    #[error("Media storage failed: {0}")]
    Storage(String),
    #[error("Media I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("Media JSON failed: {0}")]
    Json(#[from] serde_json::Error),
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// Construct only after authentication. Identity is taken from the validated
/// access token, never from request JSON. Repository is the original LFS URI.
#[derive(Debug, Clone)]
pub struct MediaScope(String);

impl MediaScope {
    pub fn new(actor: &str, repo: &str) -> Result<Self, MediaError> {
        if actor.is_empty()
            || repo.is_empty()
            || !repo.starts_with('/')
            || repo.contains('\\')
            || repo.contains('%')
            || repo.contains('?')
            || repo
                .split('/')
                .skip(1)
                .any(|p| p.is_empty() || p == "." || p == "..")
        {
            return Err(MediaError::Invalid(
                "a canonical repository and authenticated actor are required".into(),
            ));
        }
        Ok(Self(sha256_hex(&serde_json::to_vec(&(actor, repo))?)))
    }

    fn key(&self, suffix: &str) -> ObjectKey {
        ObjectKey {
            namespace: ObjectNamespace::Media,
            key: format!("media-v1/{}/{suffix}", self.0),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Pending {
    created_at: u64,
    manifest: MediaManifest,
}

fn now() -> Result<u64, MediaError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .map_err(|e| MediaError::Storage(format!("system clock: {e}")))
}

fn storage_error(e: impl std::fmt::Display) -> MediaError {
    MediaError::Storage(e.to_string())
}

async fn read_bounded(
    service: &LfsService,
    key: &ObjectKey,
    limit: usize,
) -> Result<Vec<u8>, MediaError> {
    if !service
        .obj_storage
        .inner
        .exists(key)
        .await
        .map_err(storage_error)?
    {
        return Err(MediaError::NotFound);
    }
    let (mut stream, _) = service
        .obj_storage
        .inner
        .get_stream(key)
        .await
        .map_err(storage_error)?;
    let mut data = Vec::new();
    while let Some(bytes) = stream.next().await {
        let bytes = bytes?;
        if bytes.len() > limit.saturating_sub(data.len()) {
            return Err(MediaError::Invalid(
                "stored media payload exceeds its size limit".into(),
            ));
        }
        data.extend_from_slice(&bytes);
    }
    Ok(data)
}

async fn put(service: &LfsService, key: &ObjectKey, data: Vec<u8>) -> Result<(), MediaError> {
    let size = data.len() as i64;
    service
        .obj_storage
        .inner
        .put_stream(
            key,
            data.into_stream(),
            ObjectMeta {
                size,
                ..Default::default()
            },
        )
        .await
        .map_err(storage_error)
}

async fn pending(
    service: &LfsService,
    scope: &MediaScope,
    id: &str,
) -> Result<MediaManifest, MediaError> {
    if !valid_hash(id) {
        return Err(MediaError::NotFound);
    }
    let data = read_bounded(
        service,
        &scope.key(&format!("pending/{id}")),
        protocol::MAX_MANIFEST_SIZE + 1024,
    )
    .await?;
    let entry: Pending = serde_json::from_slice(&data)?;
    if now()?.saturating_sub(entry.created_at) > PENDING_TTL.as_secs() {
        return Err(MediaError::NotFound);
    }
    if entry.manifest.id()? != id {
        return Err(MediaError::Conflict);
    }
    Ok(entry.manifest)
}

pub async fn prepare(
    service: &LfsService,
    scope: &MediaScope,
    mut manifest: MediaManifest,
) -> Result<PrepareResponse, MediaError> {
    manifest.validate()?;
    manifest.fallback_oid = Some(manifest.media_oid.clone());
    let id = manifest.id()?;
    let mut missing = Vec::new();
    let mut seen = HashSet::new();
    for chunk in &manifest.chunks {
        if seen.insert(&chunk.chunk_hash) {
            match read_chunk(service, scope, &chunk.chunk_hash, chunk.length).await {
                Ok(_) => (),
                Err(MediaError::NotFound | MediaError::Invalid(_)) => {
                    missing.push(chunk.chunk_hash.clone())
                }
                Err(e) => return Err(e),
            }
        }
    }
    let data = serde_json::to_vec(&Pending {
        created_at: now()?,
        manifest,
    })?;
    if data.len() > protocol::MAX_MANIFEST_SIZE {
        return Err(MediaError::Invalid("manifest too large".into()));
    }
    put(service, &scope.key(&format!("pending/{id}")), data).await?;
    Ok(PrepareResponse {
        manifest_id: id,
        missing_chunks: missing,
    })
}

async fn read_chunk(
    service: &LfsService,
    scope: &MediaScope,
    hash: &str,
    length: u64,
) -> Result<Vec<u8>, MediaError> {
    let bytes = read_bounded(
        service,
        &scope.key(&format!("chunks/{hash}")),
        chunker::MAX_SIZE,
    )
    .await?;
    if bytes.len() as u64 != length || sha256_hex(&bytes) != hash {
        return Err(MediaError::Invalid("chunk size or SHA-256 mismatch".into()));
    }
    Ok(bytes)
}

pub async fn upload_chunk(
    service: &LfsService,
    scope: &MediaScope,
    id: &str,
    hash: &str,
    data: Vec<u8>,
) -> Result<(), MediaError> {
    let manifest = pending(service, scope, id).await?;
    let chunk = manifest
        .chunks
        .iter()
        .find(|c| c.chunk_hash == hash)
        .ok_or(MediaError::NotFound)?;
    if data.len() as u64 != chunk.length || sha256_hex(&data) != hash {
        return Err(MediaError::Invalid("chunk size or SHA-256 mismatch".into()));
    }
    put(service, &scope.key(&format!("chunks/{hash}")), data).await
}

pub async fn get_manifest(
    service: &LfsService,
    scope: &MediaScope,
    oid: &str,
) -> Result<ManifestResponse, MediaError> {
    if !valid_hash(oid) {
        return Err(MediaError::NotFound);
    }
    let bytes = read_bounded(
        service,
        &scope.key(&format!("finalized/{oid}")),
        protocol::MAX_MANIFEST_SIZE,
    )
    .await?;
    let manifest: MediaManifest = serde_json::from_slice(&bytes)?;
    if manifest.media_oid != oid {
        return Err(MediaError::Conflict);
    }
    Ok(ManifestResponse {
        manifest_id: manifest.id()?,
        manifest,
    })
}

pub async fn download_chunk(
    service: &LfsService,
    scope: &MediaScope,
    oid: &str,
    hash: &str,
) -> Result<Bytes, MediaError> {
    let response = get_manifest(service, scope, oid).await?;
    let chunk = response
        .manifest
        .chunks
        .iter()
        .find(|c| c.chunk_hash == hash)
        .ok_or(MediaError::NotFound)?;
    Ok(Bytes::from(
        read_chunk(service, scope, hash, chunk.length).await?,
    ))
}

/// Reconstruct into a temporary file, verify every chunk, full SHA-256 and the
/// frozen CDC boundaries, persist the standard fallback, then publish metadata.
/// Publication is an atomic single-object PUT. Canonical content IDs and frozen
/// chunking make concurrent valid finalizations identical (no last-writer loss).
pub async fn finalize(
    service: &LfsService,
    scope: &MediaScope,
    id: &str,
) -> Result<(), MediaError> {
    let _permit = FINALIZERS.acquire().await.map_err(storage_error)?;
    let manifest = pending(service, scope, id).await?;
    if let Some(meta) = service
        .lfs_storage
        .get_lfs_object(&manifest.media_oid)
        .await
        .map_err(storage_error)?
        && meta.size != manifest.media_size as i64
    {
        return Err(MediaError::Conflict);
    }
    match get_manifest(service, scope, &manifest.media_oid).await {
        Ok(existing) if existing.manifest_id != id => return Err(MediaError::Conflict),
        Ok(_) | Err(MediaError::NotFound) => (),
        Err(e) => return Err(e),
    }
    let temp = tempfile::tempfile()?;
    let mut file = tokio::fs::File::from_std(temp);
    let mut digest = Sha256::new();
    for chunk in &manifest.chunks {
        let bytes = read_chunk(service, scope, &chunk.chunk_hash, chunk.length).await?;
        digest.update(&bytes);
        file.write_all(&bytes).await?;
    }
    if hex::encode(digest.finalize()) != manifest.media_oid {
        return Err(MediaError::Invalid("full media SHA-256 mismatch".into()));
    }
    file.flush().await?;
    file.rewind().await?;
    let mut file = file.into_std().await;
    let (mut file, chunks) = tokio::task::spawn_blocking(move || {
        let chunks = chunker::chunk_reader(&mut file)?;
        Ok::<_, std::io::Error>((file, chunks))
    })
    .await
    .map_err(storage_error)??;
    if chunks.len() != manifest.chunks.len()
        || chunks.iter().zip(&manifest.chunks).any(|(a, b)| {
            a.offset != b.offset || a.length != b.length || a.chunk_hash != b.chunk_hash
        })
    {
        return Err(MediaError::Invalid(
            "manifest does not use frozen fastcdc-v1 boundaries".into(),
        ));
    }
    std::io::Seek::rewind(&mut file)?;
    let stream: ObjectByteStream = Box::pin(tokio_util::io::ReaderStream::new(
        tokio::fs::File::from_std(file),
    ));
    let key = ObjectKey {
        namespace: ObjectNamespace::Lfs,
        key: manifest.media_oid.clone(),
    };
    service
        .obj_storage
        .inner
        .put_stream_bounded(
            &key,
            stream,
            ObjectMeta {
                size: manifest.media_size as i64,
                ..Default::default()
            },
        )
        .await
        .map_err(storage_error)?;
    service
        .lfs_storage
        .new_lfs_object(callisto::lfs_objects::Model {
            oid: manifest.media_oid.clone(),
            size: manifest.media_size as i64,
            exist: true,
        })
        .await
        .map_err(storage_error)?;
    let meta = service
        .lfs_storage
        .get_lfs_object(&manifest.media_oid)
        .await
        .map_err(storage_error)?
        .ok_or_else(|| MediaError::Storage("LFS fallback metadata was not persisted".into()))?;
    if meta.size != manifest.media_size as i64 {
        return Err(MediaError::Conflict);
    }
    put(
        service,
        &scope.key(&format!("finalized/{}", manifest.media_oid)),
        serde_json::to_vec(&manifest)?,
    )
    .await
}
