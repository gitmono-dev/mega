//! Offline object migration: LocalFileSystem → Amazon S3.
//!
//! This binary scans a **local object storage directory** (the same layout used by Mega's
//! `ObjectNamespace::{Git,Lfs,Log}` sharding paths) and uploads all objects to an **S3 bucket**.
//!
//! It is intended for **one-time / offline** backfill or migration jobs (e.g. moving from
//! the local backend to S3). It is *not* a continuous sync tool.
//!
//! ## What it does
//! - Lists all objects under the configured local root directory.
//! - For each object:
//!   - Performs an S3 `HEAD` request and **skips** the upload if the object already exists.
//!   - Otherwise reads the local object and uploads it to S3.
//! - Runs uploads concurrently (configurable).
//!
//! ## What it does NOT do
//! - It does **not delete** local objects after upload.
//! - It does **not overwrite** objects on S3 (it skips when `HEAD` succeeds).
//! - It does **not** perform checksums/ETag validation between local and S3.
//! - It does **not** retry uploads; any failure aborts the migration.
//!
//! ## Safety / idempotency
//! The migration is **idempotent** in the sense that re-running it will skip any objects that
//! already exist on S3 (based on `HEAD` success). This avoids accidental overwrites.
//! If you need strict "exists but different content" detection, add a `head`-then-compare
//! strategy (size/etag/checksum) before skipping.
//!
//! ## Performance notes
//! - Concurrency is controlled via the `MIGRATE_CONCURRENCY` environment variable.
//! - Objects are uploaded using **multipart streaming** to avoid buffering the full object in memory.
//! - The Tokio runtime is `current_thread`; concurrency here is mostly I/O concurrency.
//!
//! ## Configuration
//! This tool reads Mega config and uses:
//! - `object_storage.local.root_dir` as the source directory
//! - `object_storage.s3.{region,bucket,access_key_id,secret_access_key}` as the destination
//!
//! The config path resolution order is:
//! 1. CLI: `--config <path>`
//! 2. Environment: `MEGA_CONFIG=<path>`
//!
//! ## Usage
//!
//! From the workspace root:
//!
//! ```bash
//! # Basic (explicit config path)
//! cargo run -p io-orbit --bin migrate-local-to-s3 -- --config ./config.toml
//!
//! # Or via env var
//! MEGA_CONFIG=./config.toml cargo run -p io-orbit --bin migrate-local-to-s3
//!
//! # Tune concurrency (default: 16)
//! MIGRATE_CONCURRENCY=32 MEGA_CONFIG=./config.toml cargo run -p io-orbit --bin migrate-local-to-s3
//! ```
//!
//! ## Verification tips
//! - Watch logs for `skip existing object on s3:` and `migrating object:` lines.
//! - Consider checking a sample of keys on S3 (e.g. `aws s3api head-object`) after completion.
//! - If you run S3-compatible storage (MinIO/rustfs), this binary currently uses the AWS S3
//!   builder without a custom endpoint; adapt `AmazonS3Builder` accordingly if needed.

use std::{path::PathBuf, sync::Arc};

use common::{
    config::{
        Config, ObjectStorageBackend,
        loader::{ConfigInput, ConfigLoader},
    },
    errors::MegaError,
};
use futures::{StreamExt, TryStreamExt};
use object_store::{ObjectStore, ObjectStoreExt, aws::AmazonS3Builder, local::LocalFileSystem};
use tokio::sync::Semaphore;
use tracing::{error, info};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("migration failed: {}", e);
        std::process::exit(1);
    }
}

async fn run() -> Result<(), MegaError> {
    init_tracing();

    let config_path = load_config_path()?;
    let config = Config::new(
        config_path
            .to_str()
            .ok_or_else(|| MegaError::Other("config path is not valid UTF-8".to_string()))?,
    )?;

    let object_cfg = config.object_storage.clone();
    let backend = object_cfg.storage_type;

    // Build Local backend (source)
    let local_root = object_cfg.local.root_dir.clone();
    let local = LocalFileSystem::new_with_prefix(&local_root)
        .map_err(|e| MegaError::Other(format!("failed to init LocalFileSystem: {e}")))?;
    let local = Arc::new(local);

    // Build S3 backend (target) according to object_storage.storage_type:
    // - S3           -> real AWS S3 (no custom endpoint)
    // - S3Compatible -> S3-compatible service (RustFS/MinIO etc.) using endpoint_url
    let s3_cfg = object_cfg.s3.clone();
    let s3 = match backend {
        ObjectStorageBackend::S3 => AmazonS3Builder::new()
            .with_region(&s3_cfg.region)
            .with_bucket_name(&s3_cfg.bucket)
            .with_access_key_id(&s3_cfg.access_key_id)
            .with_secret_access_key(&s3_cfg.secret_access_key)
            .build()
            .map_err(|e| MegaError::Other(format!("failed to init S3 client: {e}")))?,
        ObjectStorageBackend::S3Compatible => AmazonS3Builder::new()
            .with_region(&s3_cfg.region)
            .with_bucket_name(&s3_cfg.bucket)
            .with_access_key_id(&s3_cfg.access_key_id)
            .with_secret_access_key(&s3_cfg.secret_access_key)
            .with_endpoint(&s3_cfg.endpoint_url)
            .with_allow_http(true)
            .with_virtual_hosted_style_request(false)
            .build()
            .map_err(|e| MegaError::Other(format!("failed to init S3-compatible client: {e}")))?,
        other => {
            return Err(MegaError::Other(format!(
                "migrate-local-to-s3 only supports S3/S3Compatible targets, got {:?}",
                other
            )));
        }
    };
    let s3 = Arc::new(s3);

    info!(
        "starting offline migration from local {:?} to s3 bucket {:?}",
        local_root, s3_cfg.bucket
    );

    // Concurrency for in-flight uploads. Higher isn't always better: S3 request rate limits
    // and local disk throughput will cap effective concurrency.
    let concurrency = std::env::var("MIGRATE_CONCURRENCY")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|&v| v > 0)
        .unwrap_or(16);

    migrate_all(local, s3, concurrency).await
}

fn init_tracing() {
    // Simple stderr logger; we don't depend on global app logging here.
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(true)
        .try_init();
}

fn load_config_path() -> Result<PathBuf, MegaError> {
    let mut cli_path: Option<PathBuf> = None;

    // Very lightweight CLI parsing:
    // - supports optional `--config <path>`
    // - ignores unknown args (prints a warning)
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--config" {
            if let Some(p) = args.next() {
                cli_path = Some(PathBuf::from(p));
            }
        } else {
            eprintln!("warning: unknown argument `{}` is ignored", arg);
        }
    }

    let input = ConfigInput {
        cli_path,
        env_path: std::env::var_os("MEGA_CONFIG").map(PathBuf::from),
    };

    let loaded = ConfigLoader::new(input)
        .load()
        .map_err(|e| MegaError::Other(format!("failed to load config path: {e}")))?;

    Ok(loaded.path)
}

async fn migrate_all(
    local: Arc<LocalFileSystem>,
    s3: Arc<object_store::aws::AmazonS3>,
    concurrency: usize,
) -> Result<(), MegaError> {
    let semaphore = Arc::new(Semaphore::new(concurrency));

    // List everything under the local root. Passing `None` means "from the root prefix".
    let mut listed = local.list(None);
    // Bounded collection of join handles to avoid unbounded memory growth when
    // migrating very large object stores. We cap the number of stored handles
    // to a small multiple of the concurrency limit.
    let mut tasks = Vec::new();

    while let Some(entry) = listed.next().await {
        let meta = match entry {
            Ok(m) => m,
            Err(e) => {
                error!("list local objects failed: {e}");
                return Err(MegaError::Other(format!("list local objects failed: {e}")));
            }
        };

        let path = meta.location.clone();
        let permit = semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|e| MegaError::Other(format!("failed to acquire semaphore: {e}")))?;

        let local_clone = local.clone();
        let s3_clone = s3.clone();

        tasks.push(tokio::spawn(async move {
            let _permit = permit;

            // Skip if already exists on S3.
            //
            // NOTE:
            // - This makes the migration re-runnable and avoids overwriting.
            // - If the object exists but has different content, this tool will not detect it.
            //   Add a `head`-then-compare strategy if you need that guarantee.
            if s3_clone.head(&path).await.is_ok() {
                info!("skip existing object on s3: {}", path);
                return Ok::<(), MegaError>(());
            }

            info!("migrating object: {}", path);

            // Read the local object.
            let obj = local_clone.get(&path).await.map_err(|e| {
                MegaError::Other(format!("failed to read local object {}: {e}", path))
            })?;

            // Stream upload to S3 using multipart.
            //
            // Why multipart:
            // - avoids buffering the whole object in memory
            // - works well for large objects
            //
            // Notes:
            // - if any part upload fails, we abort the multipart upload to avoid leaving
            //   dangling uploads on the backend.
            let mut upload = s3_clone.put_multipart(&path).await.map_err(|e| {
                MegaError::Other(format!("failed to init multipart upload {}: {e}", path))
            })?;

            let mut stream = obj.into_stream();
            let res: Result<(), MegaError> = async {
                while let Some(chunk) = stream.try_next().await.map_err(|e| {
                    MegaError::Other(format!("failed to read local stream chunk {}: {e}", path))
                })? {
                    upload.put_part(chunk.into()).await.map_err(|e| {
                        MegaError::Other(format!(
                            "failed to upload multipart part for {}: {e}",
                            path
                        ))
                    })?;
                }

                upload.complete().await.map_err(|e| {
                    MegaError::Other(format!("failed to complete multipart upload {}: {e}", path))
                })?;

                Ok(())
            }
            .await;

            if let Err(ref e) = res {
                // Best-effort abort; if abort fails, surface abort error as well.
                if let Err(abort_err) = upload.abort().await {
                    return Err(MegaError::Other(format!(
                        "multipart upload failed for {}: {}; abort also failed: {}",
                        path, e, abort_err
                    )));
                }
            }
            res?;

            Ok(())
        }));

        // Periodically await some tasks to keep the number of in-memory
        // JoinHandles bounded. Semaphore still enforces the true I/O
        // concurrency; this only caps bookkeeping overhead.
        if tasks.len() >= concurrency.saturating_mul(4).max(64)
            && let Some(t) = tasks.pop()
        {
            t.await
                .map_err(|e| MegaError::Other(format!("migration task panicked: {e}")))??;
        }
    }

    for t in tasks {
        t.await
            .map_err(|e| MegaError::Other(format!("migration task panicked: {e}")))??;
    }

    info!("offline migration completed");
    Ok(())
}
