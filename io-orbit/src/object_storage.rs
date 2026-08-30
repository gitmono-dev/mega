use std::{collections::HashMap, fmt, pin::Pin, time::Duration};

use bytes::Bytes;
use common::errors::MegaError;
use futures::{Stream, StreamExt, TryStreamExt};
use reqwest::Method;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ObjectKey {
    pub namespace: ObjectNamespace,

    // hash: String,
    /// content hash / logical path
    /// - git: sha1/sha256
    /// - lfs: sha256
    /// - log: path like 2025/03/worker.log
    pub key: String,
}

impl ObjectKey {
    pub fn default_sharding(&self) -> String {
        let id = &self.key;
        if id.len() < 6 {
            // For short keys, don't shard or use a different strategy
            return format!("{}/{}", self.namespace, id);
        }
        format!(
            "{}/{}/{}/{}/{}",
            self.namespace,
            &id[0..2],
            &id[2..4],
            &id[4..6],
            &id[6..]
        )
    }

    pub fn to_object_store_path(&self) -> object_store::path::Path {
        object_store::path::Path::from(self.default_sharding())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ObjectNamespace {
    Git,
    Lfs,
    /// Private media manifests and chunks, separate from public complete LFS objects.
    Media,
    Log,
    /// Artifact protocol objects (`docs/artifacts-protocol.md`), keyed by UUID string.
    Artifact,
}

impl ObjectNamespace {
    fn as_str(&self) -> &'static str {
        match self {
            ObjectNamespace::Git => "git",
            ObjectNamespace::Lfs => "lfs",
            ObjectNamespace::Media => "media",
            ObjectNamespace::Log => "log",
            ObjectNamespace::Artifact => "artifact",
        }
    }
}

impl fmt::Display for ObjectNamespace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Default)]
pub struct ObjectMeta {
    pub size: i64,
    pub checksum: Option<String>,
    pub content_type: Option<String>,
    /// （ETag / storage-class / custom）
    pub extra: HashMap<String, String>,
}

/// A streaming reader for a single object.
///
/// This represents the raw byte stream of an object, delivered incrementally.
/// Each item in the stream is a chunk of bytes.
///
/// Design notes:
/// - Uses streaming instead of `Vec<u8>` to avoid loading large objects into memory.
/// - Suitable for large blobs, pack files, or any content-addressed storage.
/// - The stream must be fully consumed by the caller.
pub type ObjectByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>;

/// A streaming source of multiple objects.
///
/// Each item yields:
/// - `ObjectKey`: identifies the object
/// - `ObjectByteStream`: streaming reader for the object's data
/// - `ObjectMeta`: metadata associated with the object
///
/// Design notes:
/// - Objects are produced lazily and may arrive out of order.
/// - Errors are propagated per-object using `Result`.
/// - This abstraction allows batching and backpressure-aware pipelines.
pub type MultiObjectByteStream<'a> = Pin<
    Box<
        dyn Stream<Item = Result<(ObjectKey, ObjectByteStream, ObjectMeta), MegaError>> + Send + 'a,
    >,
>;

#[async_trait::async_trait]
pub trait MegaObjectStorage: Send + Sync {
    // fn as_any(&self) -> &dyn Any;

    /// Whether presigned GET/PUT URLs can be generated (e.g. S3/GCS). Local disk returns `false`.
    fn supports_presigned_urls(&self) -> bool {
        false
    }

    /// Upload a single object to the storage backend.
    ///
    /// # Parameters
    /// - `key`: Logical identifier of the object.
    /// - `reader`: Streaming reader providing the object contents.
    /// - `meta`: Object metadata (size, content type, checksums, etc).
    ///
    /// # Semantics
    /// - The implementation should consume the stream exactly once.
    /// - Callers should assume the stream is invalid after this call.
    /// - Implementations may buffer internally, but should prefer streaming.
    ///
    /// # Errors
    /// Returns an error if:
    /// - The upload fails
    /// - The stream produces an I/O error
    /// - Backend-specific constraints are violated
    async fn put_stream(
        &self,
        key: &ObjectKey,
        data: ObjectByteStream,
        meta: ObjectMeta,
    ) -> Result<(), MegaError>;

    /// Atomically replace an object without buffering its entire contents.
    ///
    /// Implementations must bound upload buffering independently of object size,
    /// apply backpressure to the input, and publish only after the stream succeeds.
    /// A caller should also bound each input item, since the implementation must
    /// retain that item while consuming it. Unlike `put_stream`, this operation
    /// does not inherit create-only or single-PUT upload policies.
    ///
    /// Unsupported backends must fail explicitly, never fall back to buffering
    /// the full object. The default rejects the operation without consuming data.
    async fn put_stream_bounded(
        &self,
        _key: &ObjectKey,
        _data: ObjectByteStream,
        _meta: ObjectMeta,
    ) -> Result<(), MegaError> {
        Err(MegaError::ObjStorage(
            "storage backend does not support bounded streaming writes".to_owned(),
        ))
    }

    /// Retrieve a single object from the storage backend.
    ///
    /// # Returns
    /// - A streaming reader for the object data
    /// - The object's metadata
    ///
    /// # Semantics
    /// - The returned stream must be consumed by the caller.
    /// - Metadata is returned eagerly, data is streamed lazily.
    ///
    /// # Errors
    /// Returns an error if:
    /// - The object does not exist
    /// - Access is denied
    /// - Backend I/O fails
    async fn get_stream(
        &self,
        key: &ObjectKey,
    ) -> Result<(ObjectByteStream, ObjectMeta), MegaError>;

    /// Retrieve a range of bytes from an object.
    ///
    /// # Parameters
    /// - `key`: Object identifier
    /// - `start`: Starting byte offset (inclusive)
    /// - `end`: Ending byte offset (exclusive, None means to end of file)
    ///
    /// # Returns
    /// - A streaming reader for the object data range
    /// - The object's metadata
    ///
    /// # Semantics
    /// - Uses HTTP Range requests when supported by the backend.
    /// - For backends that don't support Range requests, falls back to full download.
    /// - The returned stream must be consumed by the caller.
    ///
    /// # Errors
    /// Returns an error if:
    /// - The object does not exist
    /// - Access is denied
    /// - Backend I/O fails
    /// - Range is invalid (start >= end, or start >= file size)
    async fn get_range_stream(
        &self,
        key: &ObjectKey,
        start: u64,
        end: Option<u64>,
    ) -> Result<(ObjectByteStream, ObjectMeta), MegaError>;

    /// Check whether an object exists.
    async fn exists(&self, key: &ObjectKey) -> Result<bool, MegaError>;

    /// Generate a presigned download URL when supported by the backend.
    ///
    /// Returns `Ok(None)` if the storage does not support presigning.
    async fn signed_url(
        &self,
        key: &ObjectKey,
        method: Method,
        expires_in: Duration,
    ) -> Result<Option<String>, MegaError>;

    /// Upload multiple objects concurrently.
    ///
    /// Objects are provided as a stream, allowing the caller to:
    /// - Generate objects lazily
    /// - Avoid holding all data in memory
    /// - Integrate with upstream pipelines (e.g. Git pack encoding)
    ///
    /// # Parameters
    /// - `objects`: Stream of objects to upload
    /// - `concurrency`: Maximum number of concurrent uploads
    ///
    /// # Semantics
    /// - Uploads are executed concurrently up to `concurrency`.
    /// - If any upload fails, the operation stops and returns the error.
    /// - Partial uploads may have already completed when an error occurs.
    ///
    /// # Errors
    /// Returns the first encountered `MegaError`.
    async fn put_many(
        &self,
        objects: MultiObjectByteStream<'_>,
        concurrency: usize,
    ) -> Result<(), MegaError> {
        objects
            .try_for_each_concurrent(concurrency, |(key, stream, meta)| async move {
                self.put_stream(&key, stream, meta)
                    .await
                    // Attach key/context so callers can see *which* object failed
                    // instead of only getting a bare storage error. This avoids
                    .map_err(|e| {
                        MegaError::Other(format!(
                            "object_storage::put_many failed for key={} namespace={} error_chain:\n{}",
                            key.key,
                            key.namespace,
                            dump_error_chain(&e),
                        ))
                    })
            })
            .await
    }

    /// Retrieve multiple objects concurrently.
    ///
    /// # Parameters
    /// - `keys`: Object identifiers to fetch
    /// - `concurrency`: Maximum number of concurrent fetches
    ///
    /// # Returns
    /// A stream yielding objects as they become available.
    ///
    /// # Semantics
    /// - Objects may be yielded out of order.
    /// - Each object is fetched independently.
    /// - Errors are reported per object via `Result`.
    ///
    /// # Typical use cases
    /// - Bulk object export
    /// - Git blob streaming
    /// - Feeding downstream encoders or pack writers
    fn get_many(&self, keys: Vec<ObjectKey>, concurrency: usize) -> MultiObjectByteStream<'_> {
        Box::pin(
            futures::stream::iter(keys)
                .map(move |key| async move {
                    let (stream, meta) = self.get_stream(&key).await?;
                    Ok((key, stream, meta))
                })
                .buffer_unordered(concurrency),
        )
    }

    /// Delete the object at the specified location.
    ///
    /// # Parameters
    /// - `key`: Object identifier
    ///
    /// # Returns
    /// - `Ok(())` if the object is deleted successfully
    /// - `Err(MegaError)` if the object does not exist or deletion fails
    async fn delete(&self, key: &ObjectKey) -> Result<(), MegaError>;
}

pub fn dump_error_chain(err: &(dyn std::error::Error + 'static)) -> String {
    let mut out = String::new();
    let mut cur: Option<&dyn std::error::Error> = Some(err);
    let mut level = 0;

    while let Some(e) = cur {
        out.push_str(&format!("[{}] {:?}\n", level, e));
        cur = e.source();
        level += 1;
    }

    out
}

#[cfg(test)]
mod tests {
    use ObjectNamespace;

    use super::*;

    #[test]
    fn test_s3_key_lfs() {
        let key = ObjectKey {
            namespace: ObjectNamespace::Lfs,
            key: "abcdef1234567890".to_string(),
        };

        // Unified 3-level sharding for LFS objects.
        assert_eq!(key.default_sharding(), "lfs/ab/cd/ef/1234567890");
    }

    #[test]
    fn test_media_namespace_is_separate_from_lfs() {
        let key = ObjectKey {
            namespace: ObjectNamespace::Media,
            key: "abcdef1234567890".to_owned(),
        };
        assert_eq!(key.default_sharding(), "media/ab/cd/ef/1234567890");
    }

    #[test]
    fn test_s3_key_git() {
        let key = ObjectKey {
            namespace: ObjectNamespace::Git,
            key: "abcdef1234567890".to_string(),
        };

        assert_eq!(key.default_sharding(), "git/ab/cd/ef/1234567890");
    }

    #[test]
    fn test_s3_key_artifact_uuid() {
        let key = ObjectKey {
            namespace: ObjectNamespace::Artifact,
            key: "550e8400-e29b-41d4-a716-446655440000".to_string(),
        };
        assert_eq!(
            key.default_sharding(),
            "artifact/55/0e/84/00-e29b-41d4-a716-446655440000"
        );
    }

    #[test]
    fn test_to_object_store_path_basic() {
        let key = ObjectKey {
            namespace: ObjectNamespace::Git,
            key: "abcdef1234567890".to_string(),
        };

        let path = key.to_object_store_path();

        assert_eq!(path.as_ref(), "git/ab/cd/ef/1234567890");
    }
}
