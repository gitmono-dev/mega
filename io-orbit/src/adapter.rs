use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use bytes::{Bytes, BytesMut};
use common::errors::MegaError;
use futures::{StreamExt, TryStreamExt, stream};
use object_store::{
    ObjectStore, ObjectStoreExt, PutMode, PutOptions, PutPayload, UpdateVersion, aws::AmazonS3,
    gcp::GoogleCloudStorage, local::LocalFileSystem, signer::Signer,
};
use reqwest::Method;

use crate::{
    error::IoOrbitError,
    log_storage::{LogManifest, LogSegmentMeta, LogStorage},
    object_storage::{MegaObjectStorage, ObjectByteStream, ObjectKey, ObjectMeta, ObjectNamespace},
};

/// Strategy used for uploading objects to the underlying [`BackendStore`].
///
/// This controls whether data is sent in a single request (`SinglePut`) or
/// split into multiple parts (`Multipart`) when supported by the backend.
/// Callers select a strategy based on object size, latency requirements,
/// and backend capabilities.
pub enum UploadStrategy {
    /// Upload the object using a multipart/streaming upload when supported.
    Multipart,
    /// Upload the entire object using a single `PUT`-style request.
    SinglePut,
}

/// Adapter that exposes an [`ObjectStore`] backend through the
/// [`MegaObjectStorage`] trait.
///
/// This type holds a concrete [`BackendStore`] implementation and an
/// [`UploadStrategy`] that determines how uploads are performed. It is the
/// main integration point between Mega's storage abstraction and the
/// `object_store` crate backends such as S3, GCS, or the local filesystem.
pub struct ObjectStoreAdapter {
    /// The concrete backend store used for all object operations.
    pub store: BackendStore,
    /// The upload strategy used when writing new objects.
    pub upload_strategy: UploadStrategy,
}

// S3/GCS require non-final parts >= 5 MiB; R2 also requires equal part sizes.
const MULTIPART_PART_SIZE: usize = 8 * 1024 * 1024;

/// Supported backend implementations for object storage.
///
/// Each variant wraps a specific `object_store` backend in an [`Arc`] so that
/// a single instance can be cheaply shared across multiple adapters or tasks.
/// New backends should be added as additional enum variants.
pub enum BackendStore {
    /// Amazon S3-compatible object storage backend.
    S3(Arc<AmazonS3>),
    /// Google Cloud Storage backend.
    Gcs(Arc<GoogleCloudStorage>),
    /// Local filesystem backend, primarily for development and testing.
    Local(Arc<LocalFileSystem>),
}

#[async_trait::async_trait]
impl MegaObjectStorage for ObjectStoreAdapter {
    fn supports_presigned_urls(&self) -> bool {
        matches!(&self.store, BackendStore::S3(_) | BackendStore::Gcs(_))
    }

    async fn put_stream(
        &self,
        key: &ObjectKey,
        data: ObjectByteStream,
        _meta: ObjectMeta,
    ) -> Result<(), MegaError> {
        let path = key.to_object_store_path();
        // Artifacts are keyed by UUID and must not depend on LFS/Git upload_strategy
        // (e.g. S3 often uses `Multipart` for LFS while we still need create-if-absent semantics).
        if matches!(key.namespace, ObjectNamespace::Artifact) {
            return self.put_idempotent(&path, data).await;
        }
        match (key.namespace, &self.upload_strategy) {
            (ObjectNamespace::Git, UploadStrategy::SinglePut) => {
                self.put_idempotent(&path, data).await
            }
            _ => match self.upload_strategy {
                UploadStrategy::Multipart => self.put_multipart(&path, data).await,
                UploadStrategy::SinglePut => self.put_single(&path, data).await,
            },
        }
    }

    async fn put_stream_bounded(
        &self,
        key: &ObjectKey,
        data: ObjectByteStream,
        _meta: ObjectMeta,
    ) -> Result<(), MegaError> {
        // Local, GCS and S3 implement atomic multipart publication. Do not use
        // the configured SinglePut policy, which buffers the entire object.
        self.put_multipart(&key.to_object_store_path(), data).await
    }

    async fn get_stream(
        &self,
        key: &ObjectKey,
    ) -> Result<(ObjectByteStream, ObjectMeta), MegaError> {
        let path = key.to_object_store_path();

        let stream = self
            .to_store()
            .get(&path)
            .await
            .map_err(IoOrbitError::from)?
            .into_stream();

        Ok((
            Box::pin(stream.map_err(std::io::Error::other)),
            ObjectMeta::default(),
        ))
    }

    async fn get_range_stream(
        &self,
        key: &ObjectKey,
        start: u64,
        end: Option<u64>,
    ) -> Result<(ObjectByteStream, ObjectMeta), MegaError> {
        let path = key.to_object_store_path();

        // Use object_store's Range support
        // object_store 0.13+ supports Range via GetRange

        // object_store 0.13's `get_range` takes `Range<u64>`.
        // If `end` is not provided, we resolve it via `head()` to get object size.
        let end = match end {
            Some(end) => end,
            None => {
                self.to_store()
                    .head(&path)
                    .await
                    .map_err(IoOrbitError::from)?
                    .size
            }
        };

        let bytes = self
            .to_store()
            .get_range(&path, start..end)
            .await
            .map_err(IoOrbitError::from)?;

        // `get_range` returns fully-buffered Bytes, adapt to our streaming type.
        let stream = stream::once(async move { Ok::<Bytes, std::io::Error>(bytes) });

        Ok((Box::pin(stream), ObjectMeta::default()))
    }

    async fn signed_url(
        &self,
        key: &ObjectKey,
        method: Method,
        expires_in: Duration,
    ) -> Result<Option<String>, MegaError> {
        let path = key.to_object_store_path();

        let url = match &self.store {
            BackendStore::S3(s3) => Some(
                s3.signed_url(method, &path, expires_in)
                    .await
                    .map_err(IoOrbitError::from)?
                    .to_string(),
            ),
            BackendStore::Gcs(gcs) => Some(
                gcs.signed_url(method, &path, expires_in)
                    .await
                    .map_err(IoOrbitError::from)?
                    .to_string(),
            ),
            BackendStore::Local(_) => None,
        };
        Ok(url)
    }

    async fn exists(&self, key: &ObjectKey) -> Result<bool, MegaError> {
        let path = key.to_object_store_path();
        Self::exists_in(self.to_store(), &path).await
    }

    async fn delete(&self, key: &ObjectKey) -> Result<(), MegaError> {
        let path = key.to_object_store_path();
        self.to_store()
            .delete(&path)
            .await
            .map_err(IoOrbitError::from)?;
        Ok(())
    }
}

/// Derives the manifest [`ObjectKey`] for a log identified by `key`.
/// Manifest is stored at `{key.key}/manifest`.
fn log_manifest_key(key: &ObjectKey) -> ObjectKey {
    ObjectKey {
        namespace: key.namespace,
        key: format!("{}/manifest", key.key),
    }
}

/// Derives the segment [`ObjectKey`] for a log segment `[start, end)`.
/// Segment is stored at `{log_key.key}/segments/{start}-{end}-{ts}`.
///
/// `ts` is a best-effort timestamp suffix (millis since epoch) used to avoid
/// key collisions under concurrent writers.
fn log_segment_key(log_key: &ObjectKey, start: u64, end: u64) -> ObjectKey {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_millis(0))
        .as_millis();
    ObjectKey {
        namespace: log_key.namespace,
        key: format!("{}/segments/{}-{}-{}", log_key.key, start, end, ts),
    }
}

/// Maximum size of a single log segment in bytes.
/// Segments larger than this will be split into multiple segments.
const MAX_SEGMENT_SIZE: u64 = 16 * 1024 * 1024; // 16 MB

#[async_trait::async_trait]
impl LogStorage for ObjectStoreAdapter {
    async fn append(
        &self,
        key: &ObjectKey,
        data: ObjectByteStream,
        _meta: ObjectMeta,
    ) -> Result<(), MegaError> {
        // Fast path for single writer/single thread (optimized here):
        // - No conditional writes, no retries, no cleanup (no concurrent write conflicts)
        // - Single manifest read (treat non-existent as empty), single segment write, single manifest overwrite

        let buf = Self::buffer_stream(data).await?;
        let total_len = buf.len() as u64;
        if total_len == 0 {
            return Ok(());
        }

        let mut manifest = self.load_manifest(key).await?;
        let mut current_offset = manifest.len;

        // If data exceeds MAX_SEGMENT_SIZE, split into multiple segments
        let mut remaining = buf.as_ref();
        while !remaining.is_empty() {
            let segment_size = remaining.len().min(MAX_SEGMENT_SIZE as usize);
            let segment_data = Bytes::copy_from_slice(&remaining[..segment_size]);
            remaining = &remaining[segment_size..];

            let segment_start = current_offset;
            let segment_end = current_offset + segment_size as u64;

            let seg_key = log_segment_key(key, segment_start, segment_end);
            let seg_storage_key = seg_key.key.clone();

            let stream_from_buf =
                stream::once(async move { Ok::<Bytes, std::io::Error>(segment_data) });
            self.put_stream(&seg_key, Box::pin(stream_from_buf), ObjectMeta::default())
                .await?;

            manifest.segments.push(LogSegmentMeta {
                offset: segment_start,
                len: segment_size as u64,
                key: seg_storage_key,
            });
            current_offset = segment_end;
        }

        manifest.len = current_offset;

        // Overwrite manifest (single writer doesn't need conditional write)
        let mkey = log_manifest_key(key);
        let path = mkey.to_object_store_path();
        let bytes = serde_json::to_vec(&manifest).map_err(|e| MegaError::Other(e.to_string()))?;
        let opts = PutOptions::from(PutMode::Overwrite);
        self.to_store()
            .put_opts(&path, PutPayload::from_bytes(Bytes::from(bytes)), opts)
            .await
            .map(|_| ())
            .map_err(IoOrbitError::from)
            .map_err(MegaError::from)?;
        Ok(())
    }

    async fn read_range(
        &self,
        key: &ObjectKey,
        offset: u64,
        length: u64,
    ) -> Result<ObjectByteStream, MegaError> {
        let m = self.load_manifest(key).await?;
        let want_end = offset.saturating_add(length).min(m.len);
        if offset >= want_end {
            let s = stream::once(async { Ok::<Bytes, std::io::Error>(Bytes::new()) });
            return Ok(Box::pin(s));
        }
        let mut out = BytesMut::new();
        for seg in &m.segments {
            let seg_end = seg.offset.saturating_add(seg.len);
            let overlap_start = offset.max(seg.offset);
            let overlap_end = want_end.min(seg_end);
            if overlap_start >= overlap_end {
                continue;
            }
            let seg_key = ObjectKey {
                namespace: key.namespace,
                key: seg.key.clone(),
            };
            let (mut strm, _) = self
                .get_range_stream(
                    &seg_key,
                    overlap_start - seg.offset,
                    Some(overlap_end - seg.offset),
                )
                .await?;
            while let Some(chunk) = strm.next().await {
                let c = chunk.map_err(MegaError::Io)?;
                out.extend_from_slice(&c);
            }
        }
        let bytes = out.freeze();
        let s = stream::once(async move { Ok::<Bytes, std::io::Error>(bytes) });
        Ok(Box::pin(s))
    }

    async fn read_lines_range(
        &self,
        key: &ObjectKey,
        start_line: u64,
        end_line: u64,
    ) -> Result<ObjectByteStream, MegaError> {
        // Empty or reversed range, return empty stream directly
        if start_line >= end_line {
            let s = stream::once(async { Ok::<Bytes, std::io::Error>(Bytes::new()) });
            return Ok(Box::pin(s));
        }

        let m = self.load_manifest(key).await?;
        if m.len == 0 {
            let s = stream::once(async { Ok::<Bytes, std::io::Error>(Bytes::new()) });
            return Ok(Box::pin(s));
        }

        let mut out = BytesMut::new();
        let mut current_line: u64 = 0;
        let mut partial_line: BytesMut = BytesMut::new();

        'outer: for seg in &m.segments {
            let seg_key = ObjectKey {
                namespace: key.namespace,
                key: seg.key.clone(),
            };
            let (mut strm, _) = self.get_stream(&seg_key).await?;

            while let Some(chunk) = strm.next().await {
                let chunk = chunk.map_err(MegaError::Io)?;

                // Process the chunk in slices split by '\n' to avoid
                // per-byte push overhead while preserving line semantics.
                for part in chunk.split_inclusive(|&b| b == b'\n') {
                    if part.is_empty() {
                        continue;
                    }

                    let ends_with_nl = part[part.len() - 1] == b'\n';
                    partial_line.extend_from_slice(part);

                    if ends_with_nl {
                        // Complete line (including newline)
                        if current_line >= start_line && current_line < end_line {
                            out.extend_from_slice(&partial_line);
                        }
                        current_line += 1;
                        partial_line.clear();

                        if current_line >= end_line {
                            break 'outer;
                        }
                    }
                }
            }
        }

        // If the last line doesn't end with '\n', treat it as a line
        if !partial_line.is_empty() && current_line >= start_line && current_line < end_line {
            out.extend_from_slice(&partial_line);
        }

        let bytes = out.freeze();
        let s = stream::once(async move { Ok::<Bytes, std::io::Error>(bytes) });
        Ok(Box::pin(s))
    }

    async fn append_concurrently(
        &self,
        key: &ObjectKey,
        data: ObjectByteStream,
        _meta: ObjectMeta,
    ) -> Result<(), MegaError> {
        const MAX_RETRIES: usize = 32;
        const BASE_DELAY_MS: u64 = 10;
        const MAX_DELAY_MS: u64 = 1000;

        let buf = Self::buffer_stream(data).await?;
        let total_len = buf.len() as u64;
        if total_len == 0 {
            return Ok(());
        }

        // If data exceeds MAX_SEGMENT_SIZE, split into multiple segments
        // For concurrent scenarios, we need to handle each segment in the retry loop
        let segments: Vec<Bytes> = if total_len > MAX_SEGMENT_SIZE {
            let mut segments = Vec::new();
            let mut remaining = buf.as_ref();
            while !remaining.is_empty() {
                let segment_size = remaining.len().min(MAX_SEGMENT_SIZE as usize);
                segments.push(Bytes::copy_from_slice(&remaining[..segment_size]));
                remaining = &remaining[segment_size..];
            }
            segments
        } else {
            vec![buf]
        };

        for attempt in 0..MAX_RETRIES {
            let (mut manifest, ver) = self.read_log_manifest_with_version(key).await?;
            let mut current_offset = manifest.len;
            let mut segment_keys = Vec::new();

            // Write all segments
            for segment_data in &segments {
                let segment_len = segment_data.len() as u64;
                let segment_start = current_offset;
                let segment_end = current_offset + segment_len;

                let seg_key = log_segment_key(key, segment_start, segment_end);
                let seg_storage_key = seg_key.key.clone();
                let seg_key_clone = seg_key.clone();
                segment_keys.push((seg_key_clone, seg_storage_key, segment_start, segment_len));

                let stream_from_buf = stream::once({
                    let segment_data = segment_data.clone();
                    async move { Ok::<Bytes, std::io::Error>(segment_data) }
                });
                self.put_stream(&seg_key, Box::pin(stream_from_buf), ObjectMeta::default())
                    .await?;

                current_offset = segment_end;
            }

            // Update manifest
            for (_, seg_storage_key, seg_start, seg_len) in &segment_keys {
                manifest.segments.push(LogSegmentMeta {
                    offset: *seg_start,
                    len: *seg_len,
                    key: seg_storage_key.clone(),
                });
            }
            manifest.len = current_offset;

            match self
                .write_log_manifest_conditional(key, &manifest, ver)
                .await
            {
                Ok(()) => return Ok(()),
                Err(IoOrbitError::WriteManifestPreconditionFailed) => {
                    // Concurrent write conflict: delete all newly written segments to avoid orphaned objects, then retry
                    for (seg_key, _, _, _) in &segment_keys {
                        let _ = self.delete(seg_key).await;
                    }

                    // Exponential backoff: delay = min(BASE_DELAY_MS * 2^attempt, MAX_DELAY_MS)
                    // Add small random jitter (time-based) to avoid thundering herd effect
                    if attempt < MAX_RETRIES - 1 {
                        let delay_ms =
                            (BASE_DELAY_MS * (1u64 << attempt.min(10))).min(MAX_DELAY_MS);
                        // Add 0-10ms jitter derived from current time, so different
                        // callers are less likely to pick the same delay.
                        let jitter_ms = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .map(|d| d.subsec_millis() as u64 % 11)
                            .unwrap_or(0);
                        tokio::time::sleep(Duration::from_millis(delay_ms + jitter_ms)).await;
                    }
                    continue;
                }
                Err(e) => {
                    // Delete all written segments
                    for (seg_key, _, _, _) in &segment_keys {
                        let _ = self.delete(seg_key).await;
                    }
                    return Err(e.into());
                }
            }
        }

        Err(MegaError::Other(
            "log manifest precondition failed (retry exceeded)".to_string(),
        ))
    }

    async fn load_manifest(&self, key: &ObjectKey) -> Result<LogManifest, MegaError> {
        let mkey = log_manifest_key(key);
        let path = mkey.to_object_store_path();

        let mut s = match self.to_store().get(&path).await {
            Ok(r) => r.into_stream(),
            Err(object_store::Error::NotFound { .. }) => {
                return Ok(LogManifest {
                    len: 0,
                    segments: Vec::new(),
                });
            }
            Err(e) => return Err(IoOrbitError::from(e).into()),
        };

        let mut buf = BytesMut::new();
        while let Some(chunk) = s.next().await {
            let c = chunk
                .map_err(std::io::Error::other)
                .map_err(MegaError::Io)?;
            buf.extend_from_slice(&c);
        }
        let bytes = buf.freeze();
        serde_json::from_slice(&bytes).map_err(|e| MegaError::Other(e.to_string()))
    }

    async fn log_exists(&self, key: &ObjectKey) -> Result<bool, MegaError> {
        let mkey = log_manifest_key(key);
        self.exists(&mkey).await
    }
}

impl ObjectStoreAdapter {
    async fn exists_in(
        store: &dyn ObjectStore,
        path: &object_store::path::Path,
    ) -> Result<bool, MegaError> {
        match store.head(path).await {
            Ok(_) => Ok(true),
            Err(object_store::Error::NotFound { .. }) => Ok(false),
            Err(error) => Err(IoOrbitError::from(error).into()),
        }
    }

    fn to_store(&self) -> &dyn ObjectStore {
        let store: &dyn ObjectStore = match &self.store {
            BackendStore::S3(s3) => s3.as_ref(),
            BackendStore::Gcs(gcs) => gcs.as_ref(),
            BackendStore::Local(local) => local.as_ref(),
        };
        store
    }

    /// Buffers an [`ObjectByteStream`] into [`Bytes`]. Used for log appends (need length) and manifest handling.
    async fn buffer_stream(mut data: ObjectByteStream) -> Result<Bytes, MegaError> {
        let mut buf = BytesMut::new();
        while let Some(chunk) = data.try_next().await.map_err(MegaError::Io)? {
            buf.extend_from_slice(&chunk);
        }
        Ok(buf.freeze())
    }

    /// Loads the [`LogManifest`] for the log identified by `key` and returns its current [`UpdateVersion`]
    /// (based on the underlying store's `e_tag`/`version`), if any.
    ///
    /// Returns an empty manifest and `None` if not found.
    async fn read_log_manifest_with_version(
        &self,
        key: &ObjectKey,
    ) -> Result<(LogManifest, Option<UpdateVersion>), MegaError> {
        let mkey = log_manifest_key(key);
        let path = mkey.to_object_store_path();

        let head = match self.to_store().head(&path).await {
            Ok(h) => h,
            Err(object_store::Error::NotFound { .. }) => {
                return Ok((
                    LogManifest {
                        len: 0,
                        segments: Vec::new(),
                    },
                    None,
                ));
            }
            Err(e) => return Err(IoOrbitError::from(e).into()),
        };

        let ver = Some(UpdateVersion {
            e_tag: head.e_tag.clone(),
            version: head.version.clone(),
        });

        let mut s = self
            .to_store()
            .get(&path)
            .await
            .map_err(IoOrbitError::from)?
            .into_stream();

        let mut buf = BytesMut::new();
        while let Some(chunk) = s.next().await {
            let c = chunk
                .map_err(std::io::Error::other)
                .map_err(MegaError::Io)?;
            buf.extend_from_slice(&c);
        }
        let bytes = buf.freeze();
        let manifest: LogManifest =
            serde_json::from_slice(&bytes).map_err(|e| MegaError::Other(e.to_string()))?;
        Ok((manifest, ver))
    }

    /// Writes the [`LogManifest`] for the log identified by `key` using a conditional update.
    ///
    /// - If `ver` is `None`, uses [`PutMode::Create`]
    /// - If `ver` is `Some`, uses [`PutMode::Update`] and fails with `Precondition` on version mismatch
    async fn write_log_manifest_conditional(
        &self,
        key: &ObjectKey,
        m: &LogManifest,
        ver: Option<UpdateVersion>,
    ) -> Result<(), IoOrbitError> {
        let mkey = log_manifest_key(key);
        let path = mkey.to_object_store_path();
        let bytes = serde_json::to_vec(m)
            .map_err(|e| IoOrbitError::Other(MegaError::Other(e.to_string())))?;

        // For backends that support conditional writes (S3/GCS etc.), use PutMode::Update for multi-writer safety.
        // For backends like LocalFileSystem that haven't implemented Update yet, fall back to Overwrite,
        // which only guarantees single-writer semantics (mainly used in test environments with Local).
        let mode = match (&self.store, ver) {
            (BackendStore::Local(_), Some(_v)) => PutMode::Overwrite,
            (_, Some(v)) => PutMode::Update(v),
            (_, None) => PutMode::Create,
        };
        let opts = PutOptions::from(mode);

        self.to_store()
            .put_opts(&path, PutPayload::from_bytes(Bytes::from(bytes)), opts)
            .await
            .map(|_| ())
            .map_err(|e| match e {
                object_store::Error::Precondition { .. }
                | object_store::Error::AlreadyExists { .. } => {
                    IoOrbitError::WriteManifestPreconditionFailed
                }
                other => IoOrbitError::from(other),
            })
    }

    async fn put_multipart(
        &self,
        path: &object_store::path::Path,
        data: ObjectByteStream,
    ) -> Result<(), MegaError> {
        Self::put_multipart_to(self.to_store(), path, data).await
    }

    /// Use one fixed-size upload buffer and await each part before reading more.
    /// The backend publishes the object atomically on `complete`; a backend that
    /// rejects multipart is an error, not a request to buffer the whole object.
    async fn put_multipart_to(
        store: &dyn ObjectStore,
        path: &object_store::path::Path,
        mut data: ObjectByteStream,
    ) -> Result<(), MegaError> {
        // An empty stream must use an empty PUT: S3 cannot complete a multipart
        // upload with no parts. Ignore zero-length input items for this decision.
        let first = loop {
            match data.try_next().await? {
                Some(chunk) if chunk.is_empty() => continue,
                Some(chunk) => break chunk,
                None => {
                    store
                        .put(path, PutPayload::new())
                        .await
                        .map_err(IoOrbitError::from)?;
                    return Ok(());
                }
            }
        };
        let mut upload = store
            .put_multipart(path)
            .await
            .map_err(IoOrbitError::from)?;

        let res = async {
            let mut buffer = BytesMut::with_capacity(MULTIPART_PART_SIZE);
            let mut chunk = first;
            loop {
                let mut remaining = chunk.as_ref();
                while !remaining.is_empty() {
                    let take = remaining.len().min(MULTIPART_PART_SIZE - buffer.len());
                    buffer.extend_from_slice(&remaining[..take]);
                    remaining = &remaining[take..];
                    if buffer.len() == MULTIPART_PART_SIZE {
                        upload
                            .put_part(std::mem::take(&mut buffer).freeze().into())
                            .await
                            .map_err(IoOrbitError::from)?;
                        buffer.reserve(MULTIPART_PART_SIZE);
                    }
                }
                match data.try_next().await? {
                    Some(next) => chunk = next,
                    None => break,
                }
            }
            if !buffer.is_empty() {
                upload
                    .put_part(buffer.freeze().into())
                    .await
                    .map_err(IoOrbitError::from)?;
            }

            upload.complete().await.map_err(IoOrbitError::from)?;

            Ok::<(), MegaError>(())
        }
        .await;

        if let Err(error) = res {
            if let Err(abort_error) = upload.abort().await {
                return Err(MegaError::ObjStorage(format!(
                    "{error}; failed to abort multipart upload: {abort_error}"
                )));
            }
            return Err(error);
        }

        Ok(())
    }

    /// Upload an object using a *single PUT* request.
    ///
    /// Why this method exists:
    ///
    /// object_store 0.13 changed the semantics of `ObjectStore::put`:
    /// - `put` no longer accepts a streaming body
    /// - it requires a fully-buffered `PutPayload`
    ///
    /// This helper adapts our internal `ObjectByteStream` abstraction
    /// (used throughout Mega for streaming object data)
    /// into a buffered upload suitable for backends that:
    /// - do NOT reliably support multipart upload (e.g. rustfs, some MinIO setups)
    /// - or where the object size is small enough to fit comfortably in memory
    ///
    /// Design trade-offs:
    /// - This method **buffers the entire object in memory**
    /// - It should ONLY be used for:
    ///   - small objects
    ///   - metadata-like payloads
    ///   - backends without stable multipart support
    ///
    /// For large objects (Git packfiles, LFS blobs, etc.),
    /// `put_stream_bounded` MUST be used instead when bounded memory is required.
    async fn put_single(
        &self,
        path: &object_store::path::Path,
        mut data: ObjectByteStream,
    ) -> Result<(), MegaError> {
        let mut buf = BytesMut::new();

        while let Some(chunk) = data.try_next().await? {
            buf.extend_from_slice(&chunk);
        }

        self.to_store()
            .put(path, PutPayload::from_bytes(buf.into()))
            .await
            .map_err(IoOrbitError::from)?;

        Ok(())
    }

    /// Uploads an object using a single `PUT` in **create-only** mode.
    ///
    /// This helper is currently used only for **Git objects** (blob/pack data)
    /// via `put_stream` when `ObjectNamespace::Git` + `UploadStrategy::SinglePut`
    /// are selected.
    ///
    /// Semantics:
    /// - Uses [`PutMode::Create`], so the backend will fail if the key already exists.
    /// - This makes writes *idempotent* for content-addressed Git blobs: the first
    ///   successful upload wins, and later attempts do not silently overwrite data.
    /// - Callers must ensure that `path` is a content-hash-based key (Git object id),
    ///   so that "already exists" is expected and safe to ignore at higher layers.
    async fn put_idempotent(
        &self,
        path: &object_store::path::Path,
        mut data: ObjectByteStream,
    ) -> Result<(), MegaError> {
        let mut buf = BytesMut::new();
        while let Some(chunk) = data.try_next().await? {
            buf.extend_from_slice(&chunk);
        }

        // Use `PutMode::Create` so we never overwrite an existing object.
        // For Git blobs (content-addressed by hash), an "already exists"
        // error is expected and treated as success by higher layers.
        match self
            .to_store()
            .put_opts(
                path,
                PutPayload::from_bytes(buf.into()),
                PutOptions::from(PutMode::Create),
            )
            .await
        {
            Ok(_) => Ok(()),
            Err(object_store::Error::AlreadyExists { .. }) => Ok(()),
            Err(e) => Err(IoOrbitError::from(e).into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{fmt, io, sync::Mutex};

    use futures::stream::BoxStream;
    use object_store::{
        CopyOptions, GetOptions, GetResult, ListResult, MultipartUpload, PutMultipartOptions,
        PutResult, UploadPart, memory::InMemory, path::Path,
    };

    use super::*;

    #[derive(Debug, Default)]
    struct UploadStats {
        started: usize,
        part_lengths: Vec<usize>,
        finished_parts: usize,
        single_lengths: Vec<usize>,
        complete_calls: usize,
        abort_calls: usize,
    }

    #[derive(Clone, Copy, Debug, Default)]
    struct Failures {
        head: bool,
        part: bool,
        complete: bool,
        abort: bool,
        unsupported: bool,
    }

    #[derive(Debug, Default)]
    struct StrictStore {
        inner: InMemory,
        stats: Arc<Mutex<UploadStats>>,
        failures: Failures,
    }

    impl fmt::Display for StrictStore {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("StrictStore")
        }
    }

    fn injected_error(message: &'static str) -> object_store::Error {
        object_store::Error::Generic {
            store: "StrictStore",
            source: Box::new(io::Error::other(message)),
        }
    }

    #[derive(Debug)]
    struct StrictUpload {
        inner: Box<dyn MultipartUpload>,
        stats: Arc<Mutex<UploadStats>>,
        failures: Failures,
    }

    #[async_trait::async_trait]
    impl MultipartUpload for StrictUpload {
        fn put_part(&mut self, payload: PutPayload) -> UploadPart {
            let length = payload.content_length();
            {
                let mut stats = self.stats.lock().unwrap();
                if let Some(previous) = stats.part_lengths.last() {
                    assert_eq!(*previous, MULTIPART_PART_SIZE, "short non-final part");
                }
                assert!(length > 0 && length <= MULTIPART_PART_SIZE);
                stats.part_lengths.push(length);
            }
            if self.failures.part {
                return Box::pin(async { Err(injected_error("injected part failure")) });
            }
            let part = self.inner.put_part(payload);
            let stats = Arc::clone(&self.stats);
            Box::pin(async move {
                part.await?;
                stats.lock().unwrap().finished_parts += 1;
                Ok(())
            })
        }

        async fn complete(&mut self) -> object_store::Result<PutResult> {
            self.stats.lock().unwrap().complete_calls += 1;
            if self.failures.complete {
                return Err(injected_error("injected complete failure"));
            }
            self.inner.complete().await
        }

        async fn abort(&mut self) -> object_store::Result<()> {
            self.stats.lock().unwrap().abort_calls += 1;
            if self.failures.abort {
                return Err(injected_error("injected abort failure"));
            }
            self.inner.abort().await
        }
    }

    #[async_trait::async_trait]
    impl ObjectStore for StrictStore {
        async fn put_opts(
            &self,
            location: &Path,
            payload: PutPayload,
            opts: PutOptions,
        ) -> object_store::Result<PutResult> {
            // Bounded writes may use a single PUT only for an empty object.
            assert_eq!(payload.content_length(), 0);
            self.stats.lock().unwrap().single_lengths.push(0);
            self.inner.put_opts(location, payload, opts).await
        }

        async fn put_multipart_opts(
            &self,
            location: &Path,
            opts: PutMultipartOptions,
        ) -> object_store::Result<Box<dyn MultipartUpload>> {
            self.stats.lock().unwrap().started += 1;
            if self.failures.unsupported {
                return Err(object_store::Error::NotSupported {
                    source: Box::new(io::Error::other("multipart disabled")),
                });
            }
            Ok(Box::new(StrictUpload {
                inner: self.inner.put_multipart_opts(location, opts).await?,
                stats: Arc::clone(&self.stats),
                failures: self.failures,
            }))
        }

        async fn get_opts(
            &self,
            location: &Path,
            options: GetOptions,
        ) -> object_store::Result<GetResult> {
            if self.failures.head {
                return Err(injected_error("injected head failure"));
            }
            self.inner.get_opts(location, options).await
        }

        fn delete_stream(
            &self,
            locations: BoxStream<'static, object_store::Result<Path>>,
        ) -> BoxStream<'static, object_store::Result<Path>> {
            self.inner.delete_stream(locations)
        }

        fn list(
            &self,
            prefix: Option<&Path>,
        ) -> BoxStream<'static, object_store::Result<object_store::ObjectMeta>> {
            self.inner.list(prefix)
        }

        async fn list_with_delimiter(
            &self,
            prefix: Option<&Path>,
        ) -> object_store::Result<ListResult> {
            self.inner.list_with_delimiter(prefix).await
        }

        async fn copy_opts(
            &self,
            from: &Path,
            to: &Path,
            options: CopyOptions,
        ) -> object_store::Result<()> {
            self.inner.copy_opts(from, to, options).await
        }
    }

    #[tokio::test]
    async fn exists_distinguishes_not_found_from_storage_failures() {
        let store = InMemory::new();
        let path = Path::from("object");
        assert!(!ObjectStoreAdapter::exists_in(&store, &path).await.unwrap());

        store.put(&path, PutPayload::new()).await.unwrap();
        assert!(ObjectStoreAdapter::exists_in(&store, &path).await.unwrap());

        let failing = StrictStore {
            failures: Failures {
                head: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let error = ObjectStoreAdapter::exists_in(&failing, &path)
            .await
            .unwrap_err();
        assert!(matches!(error, MegaError::ObjStorage(_)));
        assert!(error.to_string().contains("injected head failure"));
    }

    #[tokio::test]
    async fn multipart_aggregates_fixed_parts_with_backpressure() {
        let bytes = Bytes::from(
            (0..2 * MULTIPART_PART_SIZE + 17)
                .map(|index| (index as u8).wrapping_mul(31))
                .collect::<Vec<_>>(),
        );
        for length in [2 * MULTIPART_PART_SIZE, bytes.len()] {
            for input_size in [4096, bytes.len()] {
                let store = StrictStore::default();
                let path = Path::from("object");
                let expected = bytes.slice(..length);
                let input = expected.clone();
                let stats = Arc::clone(&store.stats);
                let source = stream::unfold(0, move |offset| {
                    let input = input.clone();
                    let stats = Arc::clone(&stats);
                    async move {
                        // The adapter must finish full parts before polling for
                        // more bytes, even when inputs are tiny ReaderStream items.
                        assert_eq!(
                            stats.lock().unwrap().finished_parts,
                            offset / MULTIPART_PART_SIZE
                        );
                        if offset == input.len() {
                            None
                        } else {
                            let end = (offset + input_size).min(input.len());
                            Some((Ok(input.slice(offset..end)), end))
                        }
                    }
                });
                ObjectStoreAdapter::put_multipart_to(&store, &path, Box::pin(source))
                    .await
                    .unwrap();
                assert_eq!(
                    store.get(&path).await.unwrap().bytes().await.unwrap(),
                    expected
                );
                let stats = store.stats.lock().unwrap();
                let mut lengths = vec![MULTIPART_PART_SIZE; 2];
                if length % MULTIPART_PART_SIZE != 0 {
                    lengths.push(length % MULTIPART_PART_SIZE);
                }
                assert_eq!(stats.part_lengths, lengths);
                assert_eq!(stats.complete_calls, 1);
                assert_eq!(stats.abort_calls, 0);
                assert!(stats.single_lengths.is_empty());
            }
        }
    }

    #[tokio::test]
    async fn multipart_empty_object_uses_single_put() {
        for empty_items in [0, 3] {
            let store = StrictStore::default();
            let path = Path::from("empty");
            let source = stream::iter((0..empty_items).map(|_| Ok(Bytes::new())));
            ObjectStoreAdapter::put_multipart_to(&store, &path, Box::pin(source))
                .await
                .unwrap();
            assert_eq!(store.head(&path).await.unwrap().size, 0);
            let stats = store.stats.lock().unwrap();
            assert_eq!(stats.started, 0);
            assert_eq!(stats.single_lengths, [0]);
        }
    }

    #[tokio::test]
    async fn multipart_failures_abort_and_preserve_existing_object() {
        for failures in [
            Failures::default(),
            Failures {
                part: true,
                ..Default::default()
            },
            Failures {
                complete: true,
                ..Default::default()
            },
            Failures {
                abort: true,
                ..Default::default()
            },
        ] {
            let store = StrictStore {
                failures,
                ..Default::default()
            };
            let path = Path::from("existing");
            store.inner.put(&path, "keep me".into()).await.unwrap();
            let mut items = vec![Ok(Bytes::from(vec![1; MULTIPART_PART_SIZE]))];
            if !failures.part && !failures.complete {
                items.push(Err(io::Error::other("injected stream failure")));
            }
            let error =
                ObjectStoreAdapter::put_multipart_to(&store, &path, Box::pin(stream::iter(items)))
                    .await
                    .unwrap_err()
                    .to_string();
            let phase = if failures.part {
                "part"
            } else if failures.complete {
                "complete"
            } else {
                "stream"
            };
            assert!(
                error.contains(&format!("injected {phase} failure")),
                "{error}"
            );
            if failures.abort {
                assert!(error.contains("injected abort failure"), "{error}");
            }
            assert_eq!(
                store.get(&path).await.unwrap().bytes().await.unwrap(),
                "keep me"
            );
            let stats = store.stats.lock().unwrap();
            assert_eq!(stats.abort_calls, 1);
            assert_eq!(stats.complete_calls, usize::from(failures.complete));
            assert!(stats.single_lengths.is_empty());
        }
    }

    #[tokio::test]
    async fn multipart_unsupported_does_not_fall_back_to_buffering() {
        let store = StrictStore {
            failures: Failures {
                unsupported: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let path = Path::from("unsupported");
        let source = stream::once(async { Ok(Bytes::from_static(b"data")) });
        assert!(
            ObjectStoreAdapter::put_multipart_to(&store, &path, Box::pin(source))
                .await
                .is_err()
        );
        let stats = store.stats.lock().unwrap();
        assert_eq!(stats.started, 1);
        assert!(stats.single_lengths.is_empty());
    }

    #[tokio::test]
    async fn bounded_local_upload_streams_then_atomically_replaces() {
        let dir = tempfile::tempdir().unwrap();
        let local = Arc::new(LocalFileSystem::new_with_prefix(dir.path()).unwrap());
        let adapter = ObjectStoreAdapter {
            store: BackendStore::Local(Arc::clone(&local)),
            upload_strategy: UploadStrategy::SinglePut,
        };
        let key = ObjectKey {
            namespace: ObjectNamespace::Lfs,
            key: "a".repeat(64),
        };
        let path = key.to_object_store_path();
        local.put(&path, "old object".into()).await.unwrap();
        let target = local.path_to_filesystem(&path).unwrap();
        let observed_path = path.clone();
        let observed_local = Arc::clone(&local);
        let observed_parent = target.parent().unwrap().to_path_buf();
        let first = stream::once(async { Ok(Bytes::from(vec![7; MULTIPART_PART_SIZE])) });
        let last = stream::once(async move {
            assert_eq!(
                observed_local
                    .get(&observed_path)
                    .await
                    .unwrap()
                    .bytes()
                    .await
                    .unwrap(),
                "old object"
            );
            // A full part has already reached the staging file before EOF;
            // SinglePut buffering would leave only the original small file.
            assert!(std::fs::read_dir(observed_parent).unwrap().any(|entry| {
                // Inspect an open handle, not a potentially stale directory
                // entry's cached length while the file is still open.
                std::fs::File::open(entry.unwrap().path())
                    .unwrap()
                    .metadata()
                    .unwrap()
                    .len()
                    == MULTIPART_PART_SIZE as u64
            }));
            Ok(Bytes::from_static(b"tail"))
        });
        adapter
            .put_stream_bounded(&key, Box::pin(first.chain(last)), ObjectMeta::default())
            .await
            .unwrap();
        let expected = local.get(&path).await.unwrap().bytes().await.unwrap();
        assert_eq!(
            &expected[..MULTIPART_PART_SIZE],
            vec![7; MULTIPART_PART_SIZE]
        );
        assert_eq!(&expected[MULTIPART_PART_SIZE..], b"tail");

        let failed = stream::iter(vec![
            Ok(Bytes::from(vec![3; MULTIPART_PART_SIZE])),
            Err(io::Error::other("injected stream failure")),
        ]);
        assert!(
            adapter
                .put_stream_bounded(&key, Box::pin(failed), ObjectMeta::default())
                .await
                .is_err()
        );
        assert_eq!(
            local.get(&path).await.unwrap().bytes().await.unwrap(),
            expected
        );
        assert_eq!(
            std::fs::read_dir(target.parent().unwrap()).unwrap().count(),
            1
        );
    }
}
