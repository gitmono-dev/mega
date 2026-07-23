//! Antares filesystem management via scorpiofs direct calls.
//!
//! This module provides a singleton wrapper around `scorpiofs::AntaresManager`
//! for managing overlay filesystem mounts used during build operations.
//!
//! Platform support:
//!
//! Orion integrates with the FUSE filesystem via `scorpiofs`, which is only used
//! on Linux in this repository. To keep local development tooling usable on
//! macOS/Windows, this module provides:
//!
//! - **Linux**: real implementation backed by `scorpiofs`
//! - **Non-Linux**: stub implementation (compiles; mount/unmount return errors)

#[cfg(target_os = "linux")]
mod imp {
    //! Linux implementation backed by `scorpiofs`.

    use std::{
        any::Any,
        collections::HashMap,
        error::Error,
        io,
        panic::AssertUnwindSafe,
        path::{Component, Path, PathBuf},
        sync::{Arc, LazyLock},
        time::Duration,
    };

    use futures_util::FutureExt;
    use reqwest::Client;
    use scorpiofs::{AntaresConfig, AntaresManager, AntaresPaths, antares::fuse::AntaresFuse};
    use serde::Deserialize;
    use tokio::{
        fs,
        io::AsyncWriteExt,
        process::Command,
        sync::{Mutex, OnceCell},
    };
    use uuid::Uuid;

    static MANAGER: OnceCell<Arc<AntaresManager>> = OnceCell::const_new();
    static DIRECT_CL_MOUNTS: LazyLock<Mutex<HashMap<String, DirectClMount>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));
    const TEST_BROWSE_JOB_ID: &str = "antares_test";

    type DynError = Box<dyn Error + Send + Sync>;

    #[derive(Debug, Deserialize)]
    struct CommonResult<T> {
        req_result: bool,
        data: Option<T>,
        err_message: String,
    }

    #[derive(Debug, Clone, Deserialize)]
    struct ClFileEntry {
        path: String,
        sha: String,
        action: String,
    }

    struct DirectClMount {
        config: AntaresConfig,
        fuse: AntaresFuse,
    }

    /// Get the global AntaresManager instance.
    ///
    /// Initializes the manager on first call by loading the scorpio configuration
    /// from the path specified by `SCORPIO_CONFIG` environment variable.
    ///
    /// If `SCORPIO_CONFIG` is not set, Orion will look for `scorpio.toml` in:
    /// 1. Current working directory
    /// 2. Next to the executable
    /// 3. `/etc/scorpio/scorpio.toml` (system default)
    ///
    /// Returns an error if no config file is found.
    async fn get_manager() -> Result<&'static Arc<AntaresManager>, DynError> {
        MANAGER
            .get_or_try_init(|| async {
                let config_path = resolve_config_path()?;
                let config_path_str = config_path.to_str().ok_or_else(|| -> DynError {
                    Box::new(io_other("Invalid SCORPIO_CONFIG path (non-UTF8)"))
                })?;

                tracing::info!("Initializing Antares with config: {}", config_path_str);

                if let Err(e) = scorpiofs::util::config::init_config(config_path_str) {
                    if !e.contains("already initialized") {
                        return Err(Box::new(io_other(format!(
                            "Failed to load scorpio config from {config_path_str}: {e}. \
Hint: set SCORPIO_CONFIG=/path/to/scorpio.toml or create /etc/scorpio/scorpio.toml"
                        ))) as DynError);
                    }
                    tracing::debug!("Scorpio config already initialized, using existing config");
                }

                let paths = AntaresPaths::from_global_config();
                let manager = AntaresManager::new(paths).await;
                Ok(Arc::new(manager))
            })
            .await
    }

    fn io_other(message: impl Into<String>) -> io::Error {
        io::Error::other(message.into())
    }

    /// Resolve scorpio configuration path.
    fn resolve_config_path() -> Result<PathBuf, DynError> {
        // 1. Check SCORPIO_CONFIG environment variable
        if let Ok(path) = std::env::var("SCORPIO_CONFIG") {
            let config_path = PathBuf::from(&path);
            if config_path.exists() {
                return Ok(config_path);
            }
            return Err(Box::new(io_other(format!(
                "SCORPIO_CONFIG is set but file does not exist: {}",
                config_path.display()
            ))));
        }

        // 2. Check current working directory
        let cwd = std::env::current_dir().map_err(|e| {
            Box::new(io_other(format!(
                "Failed to get current working directory: {e}"
            ))) as DynError
        })?;
        let cwd_candidate = cwd.join("scorpio.toml");
        if cwd_candidate.exists() {
            return Ok(cwd_candidate);
        }

        // 3. Check next to executable
        if let Ok(exe) = std::env::current_exe()
            && let Some(exe_dir) = exe.parent()
        {
            let exe_candidate = exe_dir.join("scorpio.toml");
            if exe_candidate.exists() {
                return Ok(exe_candidate);
            }
        }

        // 4. Check system default path
        let system_candidate = PathBuf::from("/etc/scorpio/scorpio.toml");
        if system_candidate.exists() {
            return Ok(system_candidate);
        }

        Err(Box::new(io_other(format!(
            "Scorpio config not found. Set SCORPIO_CONFIG=/path/to/scorpio.toml, \
         place scorpio.toml in the working directory ({}), \
         or create /etc/scorpio/scorpio.toml",
            cwd.display()
        ))))
    }

    /// Mount a job overlay filesystem.
    ///
    /// Creates a new Antares overlay mount for the specified job. The underlying
    /// Dicfuse layer provides read-only access to the repository, while the overlay
    /// allows copy-on-write modifications.
    ///
    /// # Arguments
    /// * `job_id` - Unique identifier for this build job
    /// * `cl` - Optional changelist layer name
    ///
    /// # Returns
    /// The `AntaresConfig` containing mountpoint and job metadata on success.
    pub async fn mount_job(
        job_id: &str,
        repo: &str,
        cl: Option<&str>,
    ) -> Result<AntaresConfig, DynError> {
        tracing::debug!(
            "Mounting Antares job: job_id={}, repo={}, cl={:?}",
            job_id,
            repo,
            cl
        );

        if let Some(cl_link) = cl {
            return mount_job_with_prepopulated_cl(job_id, repo, cl_link).await;
        }

        let mountpoint = AntaresPaths::from_global_config().mount_root.join(job_id);
        prepare_mountpoint_for_retry(job_id, &mountpoint).await?;

        let manager = get_manager().await?;
        let config = run_with_panic_guard(
            format!("Antares mount panicked for job_id={job_id}, cl={cl:?}"),
            manager.mount_job(job_id, None),
        )
        .await?;

        Ok(config)
    }

    async fn wait_for_dicfuse_ready(
        manager: &AntaresManager,
        job_id: &str,
    ) -> Result<(), DynError> {
        const DICFUSE_INIT_TIMEOUT_SECS: u64 = 120;
        let dicfuse = manager.dicfuse();
        tracing::info!(
            job_id = job_id,
            timeout_secs = DICFUSE_INIT_TIMEOUT_SECS,
            "Waiting for Dicfuse to become ready before mounting direct CL job."
        );

        match tokio::time::timeout(
            Duration::from_secs(DICFUSE_INIT_TIMEOUT_SECS),
            dicfuse.store.wait_for_ready(),
        )
        .await
        {
            Ok(_) => Ok(()),
            Err(_) => Err(Box::new(io_other(format!(
                "Dicfuse initialization timed out after {}s for direct CL job {}",
                DICFUSE_INIT_TIMEOUT_SECS, job_id
            )))),
        }
    }

    async fn mount_job_with_prepopulated_cl(
        job_id: &str,
        repo: &str,
        cl_link: &str,
    ) -> Result<AntaresConfig, DynError> {
        let manager = get_manager().await?;
        let paths = AntaresPaths::from_global_config();
        let upper_id = Uuid::new_v4().to_string();
        let cl_id = Uuid::new_v4().to_string();
        let upper_dir = paths.upper_root.join(&upper_id);
        let cl_dir = paths.cl_root.join(&cl_id);
        let mountpoint = paths.mount_root.join(job_id);

        // DEBUG: Log Antares mount path construction
        tracing::debug!(
            job_id = job_id,
            repo = repo,
            cl_link = cl_link,
            upper_id = %upper_id,
            cl_id = %cl_id,
            upper_dir = %upper_dir.display(),
            cl_dir = %cl_dir.display(),
            mountpoint = %mountpoint.display(),
            antares_mount_root = %paths.mount_root.display(),
            "DEBUG: Antares mount path construction"
        );

        prepare_mountpoint_for_retry(job_id, &mountpoint).await?;

        tokio::fs::create_dir_all(&upper_dir).await?;
        tokio::fs::create_dir_all(&mountpoint).await?;

        // DEBUG: Log before CL overlay population
        tracing::debug!(
            job_id = job_id,
            repo = repo,
            cl_link = cl_link,
            cl_dir = %cl_dir.display(),
            "DEBUG: About to populate CL overlay directory"
        );

        populate_cl_overlay_dir(job_id, repo, cl_link, &cl_dir).await?;

        // DEBUG: Verify CL overlay directory contents after population
        match tokio::fs::read_dir(&cl_dir).await {
            Ok(mut entries) => {
                let mut cl_entries = Vec::new();
                while let Some(entry) = entries.next_entry().await.unwrap_or(None) {
                    cl_entries.push(entry.path().display().to_string());
                }
                tracing::debug!(
                    job_id = job_id,
                    cl_dir = %cl_dir.display(),
                    cl_entries = ?cl_entries,
                    "DEBUG: CL overlay directory contents after population"
                );
            }
            Err(e) => {
                tracing::warn!("Failed to read CL overlay dir {}: {}", cl_dir.display(), e);
            }
        }

        wait_for_dicfuse_ready(manager.as_ref(), job_id).await?;

        let dicfuse = manager.dicfuse();
        let mut fuse = AntaresFuse::new(
            mountpoint.clone(),
            dicfuse,
            upper_dir.clone(),
            Some(cl_dir.clone()),
        )
        .await?;
        run_with_panic_guard(
            format!("Direct CL Antares mount panicked for job_id={job_id}, cl={cl_link}"),
            fuse.mount(),
        )
        .await?;

        let config = AntaresConfig {
            job_id: job_id.to_string(),
            mountpoint,
            upper_id,
            upper_dir,
            cl_dir: Some(cl_dir),
            cl_id: Some(cl_id),
        };

        let previous = DIRECT_CL_MOUNTS.lock().await.insert(
            job_id.to_string(),
            DirectClMount {
                config: config.clone(),
                fuse,
            },
        );
        if previous.is_some() {
            tracing::warn!(
                job_id = job_id,
                "Replaced an existing direct CL mount entry while mounting a new one."
            );
        }

        tracing::info!(
            job_id = job_id,
            repo = repo,
            cl_link = cl_link,
            mountpoint = %config.mountpoint.display(),
            cl_dir = %config.cl_dir.as_ref().expect("cl_dir must exist for direct CL mounts").display(),
            "Mounted direct CL Antares job with pre-populated overlay."
        );

        Ok(config)
    }

    async fn prepare_mountpoint_for_retry(job_id: &str, mountpoint: &Path) -> Result<(), DynError> {
        if let Err(err) = unmount_job(job_id).await {
            tracing::warn!(
                job_id = job_id,
                mountpoint = %mountpoint.display(),
                "Best-effort Antares unmount before remount failed: {}",
                err
            );
        }

        if !best_effort_detach_mountpoint(mountpoint, false).await
            && !best_effort_detach_mountpoint(mountpoint, true).await
        {
            return Err(Box::new(io_other(format!(
                "Failed to prepare Antares mountpoint {} for job {} because it still appears mounted after detach attempts",
                mountpoint.display(),
                job_id
            ))));
        }

        match remove_mountpoint_path(mountpoint).await {
            Ok(()) => Ok(()),
            Err(first_err) => {
                tracing::warn!(
                    job_id = job_id,
                    mountpoint = %mountpoint.display(),
                    "Mountpoint cleanup failed after regular unmount, retrying with lazy detach: {}",
                    first_err
                );
                best_effort_detach_mountpoint(mountpoint, true).await;
                remove_mountpoint_path(mountpoint).await.map_err(|err| {
                    Box::new(io_other(format!(
                        "Failed to prepare Antares mountpoint {} for job {} after retry-safe cleanup: {err}",
                        mountpoint.display(),
                        job_id
                    ))) as DynError
                })
            }
        }
    }

    async fn best_effort_detach_mountpoint(mountpoint: &Path, lazy: bool) -> bool {
        if !mountpoint.exists() {
            return true;
        }

        let flag = if lazy { "-uz" } else { "-u" };
        match Command::new("fusermount")
            .arg(flag)
            .arg(mountpoint)
            .output()
            .await
        {
            Ok(output) if output.status.success() => {
                tracing::info!(
                    mountpoint = %mountpoint.display(),
                    lazy = lazy,
                    "Detached stale Antares mountpoint with fusermount."
                );
                true
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                if fusermount_output_indicates_safe_detach(&stderr) {
                    tracing::debug!(
                        mountpoint = %mountpoint.display(),
                        lazy = lazy,
                        "fusermount reported mountpoint already detached: {}",
                        stderr.trim()
                    );
                    true
                } else {
                    tracing::warn!(
                        mountpoint = %mountpoint.display(),
                        lazy = lazy,
                        status = %output.status,
                        "fusermount detach reported stderr: {}",
                        stderr.trim()
                    );
                    false
                }
            }
            Err(err) => {
                tracing::warn!(
                    mountpoint = %mountpoint.display(),
                    lazy = lazy,
                    "Failed to execute fusermount for stale mountpoint cleanup: {}",
                    err
                );
                false
            }
        }
    }

    fn fusermount_output_indicates_safe_detach(stderr: &str) -> bool {
        stderr.contains("not mounted")
            || stderr.contains("Invalid argument")
            || stderr.contains("not found in /etc/mtab")
    }

    async fn remove_mountpoint_path(mountpoint: &Path) -> io::Result<()> {
        match fs::symlink_metadata(mountpoint).await {
            Ok(metadata) => {
                if metadata.is_dir() {
                    fs::remove_dir_all(mountpoint).await
                } else {
                    fs::remove_file(mountpoint).await
                }
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err),
        }
    }

    /// Initialize Antares during Orion startup and eagerly trigger Dicfuse import.
    ///
    /// This keeps the first build request from paying the full Dicfuse cold-start
    /// cost. Readiness waiting runs in the background so Orion can continue booting.
    #[allow(dead_code)]
    pub(crate) async fn warmup_dicfuse() -> Result<(), DynError> {
        tracing::info!("Initializing Antares Dicfuse during Orion startup");

        // NOTE: do not read scorpio config accessors before `get_manager()`.
        // `get_manager()` is what calls `scorpiofs::util::config::init_config(...)`.
        // Reading accessors first would trigger scorpiofs' lazy DEFAULT_CONFIG
        // fallback, and any values logged here would be the built-in defaults
        // (e.g. base_url=http://localhost:8000) rather than the values from
        // the scorpio.toml that init_config is about to load.
        let manager = get_manager().await?;
        let manager_for_test_mount = Arc::clone(manager);
        let dicfuse = manager.dicfuse();

        let workspace_path = scorpiofs::util::config::workspace();
        let base_url = scorpiofs::util::config::base_url();
        let store_path = scorpiofs::util::config::store_path();
        tracing::debug!(
            workspace = %workspace_path,
            base_url = %base_url,
            store_path = %store_path,
            "Dicfuse prewarm configuration"
        );

        dicfuse.start_import();

        tokio::spawn(async move {
            let warmup_timeout_secs: u64 = std::env::var("ORION_DICFUSE_WARMUP_TIMEOUT_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1200);
            tracing::info!(
                "Waiting for Antares Dicfuse warmup to finish (timeout: {}s)",
                warmup_timeout_secs
            );

            match tokio::time::timeout(
                Duration::from_secs(warmup_timeout_secs),
                dicfuse.store.wait_for_ready(),
            )
            .await
            {
                Ok(_) => {
                    tracing::info!("Antares Dicfuse warmup completed");
                    log_dicfuse_root_tree();
                    if is_test_mount_enabled() {
                        ensure_test_mount(manager_for_test_mount.as_ref()).await;
                    } else {
                        tracing::info!(
                            "Antares test mount disabled by ORION_ENABLE_ANTARES_TEST_MOUNT"
                        );
                    }
                }
                Err(_) => tracing::warn!(
                    "Antares Dicfuse warmup timed out after {}s",
                    warmup_timeout_secs
                ),
            }
        });

        Ok(())
    }

    fn is_test_mount_enabled() -> bool {
        match std::env::var("ORION_ENABLE_ANTARES_TEST_MOUNT") {
            Ok(v) => {
                let v = v.trim().to_ascii_lowercase();
                !(v == "0" || v == "false" || v == "no" || v == "off")
            }
            Err(_) => true,
        }
    }

    async fn ensure_test_mount(manager: &AntaresManager) {
        match run_with_panic_guard(
            format!("Antares test mount panicked for job_id={TEST_BROWSE_JOB_ID}"),
            manager.mount_job(TEST_BROWSE_JOB_ID, None),
        )
        .await
        {
            Ok(config) => {
                tracing::info!(
                    "Antares test mount ready: job_id={}, mountpoint={}",
                    TEST_BROWSE_JOB_ID,
                    config.mountpoint.display()
                );
            }
            Err(err) => {
                tracing::warn!(
                    "Failed to create Antares test mount job_id={}: {}",
                    TEST_BROWSE_JOB_ID,
                    err
                );
            }
        }
    }

    fn log_dicfuse_root_tree() {
        let root = PathBuf::from(scorpiofs::util::config::workspace());
        let max_depth = std::env::var("ORION_DICFUSE_ROOT_TREE_DEPTH")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(2);
        let max_entries = std::env::var("ORION_DICFUSE_ROOT_TREE_MAX_ENTRIES")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(200);

        tracing::info!(
            root = %root.display(),
            max_depth,
            max_entries,
            "Dicfuse init: printing workspace root tree"
        );

        if !root.exists() {
            tracing::warn!("Dicfuse workspace path does not exist: {}", root.display());
            return;
        }

        let mut printed = 0usize;
        tracing::info!("[dicfuse-root] /");
        log_tree_recursive(&root, &root, 0, max_depth, max_entries, &mut printed);

        if printed >= max_entries {
            tracing::info!(
                "Dicfuse root tree output truncated at {} entries (set ORION_DICFUSE_ROOT_TREE_MAX_ENTRIES to increase)",
                max_entries
            );
        }
    }

    fn cl_files_timeout() -> Duration {
        std::env::var("ORION_CL_FILES_TIMEOUT_SECS")
            .ok()
            .and_then(|raw| raw.parse::<u64>().ok())
            .filter(|secs| *secs > 0)
            .map(Duration::from_secs)
            .unwrap_or_else(|| Duration::from_secs(120))
    }

    fn http_client() -> Result<Client, DynError> {
        Client::builder()
            .timeout(cl_files_timeout())
            .build()
            .map_err(|err| {
                Box::new(io_other(format!(
                    "Failed to build CL overlay HTTP client: {err}"
                ))) as DynError
            })
    }

    async fn fetch_cl_files(cl_link: &str) -> Result<Vec<ClFileEntry>, DynError> {
        let base_url = scorpiofs::util::config::base_url();
        let url = format!("{base_url}/api/v1/cl/{cl_link}/files-list");
        let client = http_client()?;
        let response = client.get(&url).send().await.map_err(|err| {
            Box::new(io_other(format!(
                "Failed to fetch CL files from {url}: {err}"
            ))) as DynError
        })?;

        if !response.status().is_success() {
            return Err(Box::new(io_other(format!(
                "Fetching CL files from {url} failed with HTTP {}",
                response.status()
            ))));
        }

        let body: CommonResult<Vec<ClFileEntry>> = response.json().await.map_err(|err| {
            Box::new(io_other(format!(
                "Failed to parse CL files response for {cl_link}: {err}"
            ))) as DynError
        })?;

        if !body.req_result {
            return Err(Box::new(io_other(format!(
                "CL files API returned req_result=false for {cl_link}: {}",
                body.err_message
            ))));
        }

        Ok(body.data.unwrap_or_default())
    }

    fn resolve_overlay_relative_path(repo: &str, entry_path: &str) -> Result<PathBuf, DynError> {
        let repo_prefix = repo.trim_matches('/');
        let trimmed_entry = entry_path.trim().trim_start_matches('/');

        // DEBUG: Log path resolution inputs
        tracing::debug!(
            repo = %repo,
            entry_path = %entry_path,
            repo_prefix = %repo_prefix,
            trimmed_entry = %trimmed_entry,
            "DEBUG: resolve_overlay_relative_path inputs"
        );

        if trimmed_entry.is_empty() {
            return Err(Box::new(io_other(
                "CL file entry path is empty after normalization.",
            )));
        }

        let relative = if repo_prefix.is_empty()
            || trimmed_entry == repo_prefix
            || trimmed_entry
                .strip_prefix(repo_prefix)
                .is_some_and(|suffix| suffix.starts_with('/'))
        {
            trimmed_entry.to_string()
        } else {
            format!("{repo_prefix}/{trimmed_entry}")
        };

        // DEBUG: Log relative path calculation
        tracing::debug!(
            repo_prefix = %repo_prefix,
            trimmed_entry = %trimmed_entry,
            relative = %relative,
            "DEBUG: resolved overlay relative path"
        );

        let candidate = Path::new(&relative);
        if candidate
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
        {
            return Ok(candidate.to_path_buf());
        }

        Err(Box::new(io_other(format!(
            "Rejected unsafe CL overlay path: repo={repo}, entry_path={entry_path}, relative={relative}"
        ))))
    }

    async fn download_blob_to_path(
        client: &Client,
        oid: &str,
        dest: &Path,
    ) -> Result<(), DynError> {
        let base_url = scorpiofs::util::config::base_url();
        let clean_oid = oid.trim_start_matches("sha1:");
        let url = format!("{base_url}/api/v1/file/blob/{clean_oid}");

        // DEBUG: Log blob download details
        tracing::debug!(
            oid = %oid,
            clean_oid = %clean_oid,
            url = %url,
            dest = %dest.display(),
            "DEBUG: Downloading blob"
        );

        let response = client.get(&url).send().await.map_err(|err| {
            Box::new(io_other(format!(
                "Failed to download blob {clean_oid}: {err}"
            ))) as DynError
        })?;

        if !response.status().is_success() {
            return Err(Box::new(io_other(format!(
                "Downloading blob {clean_oid} failed with HTTP {}",
                response.status()
            ))));
        }

        if let Some(parent) = dest.parent() {
            tracing::debug!(dest = %dest.display(), parent = %parent.display(), "DEBUG: Creating parent directories for blob");
            tokio::fs::create_dir_all(parent).await.map_err(|err| {
                Box::new(io_other(format!(
                    "Failed to create CL overlay dir {:?}: {err}",
                    parent
                ))) as DynError
            })?;
        }

        let bytes = response.bytes().await.map_err(|err| {
            Box::new(io_other(format!(
                "Failed to read blob {clean_oid} response body: {err}"
            ))) as DynError
        })?;

        tracing::debug!(
            oid = %clean_oid,
            dest = %dest.display(),
            bytes_len = bytes.len(),
            "DEBUG: Writing blob to file"
        );

        let mut file = tokio::fs::File::create(dest).await.map_err(|err| {
            Box::new(io_other(format!(
                "Failed to create CL overlay file {:?}: {err}",
                dest
            ))) as DynError
        })?;
        file.write_all(&bytes).await.map_err(|err| {
            Box::new(io_other(format!(
                "Failed to write CL overlay file {:?}: {err}",
                dest
            ))) as DynError
        })?;

        // DEBUG: Verify file was written
        match tokio::fs::metadata(dest).await {
            Ok(meta) => {
                tracing::debug!(dest = %dest.display(), size = meta.len(), "DEBUG: Blob file written successfully");
            }
            Err(e) => {
                tracing::warn!(dest = %dest.display(), error = %e, "DEBUG: Failed to stat blob file after write");
            }
        }

        Ok(())
    }

    async fn populate_cl_overlay_dir(
        job_id: &str,
        repo: &str,
        cl_link: &str,
        cl_dir: &Path,
    ) -> Result<(), DynError> {
        if cl_dir.exists() {
            tokio::fs::remove_dir_all(cl_dir).await.map_err(|err| {
                Box::new(io_other(format!(
                    "Failed to clear CL overlay dir {:?}: {err}",
                    cl_dir
                ))) as DynError
            })?;
        }
        tokio::fs::create_dir_all(cl_dir).await.map_err(|err| {
            Box::new(io_other(format!(
                "Failed to create CL overlay dir {:?}: {err}",
                cl_dir
            ))) as DynError
        })?;

        let files = fetch_cl_files(cl_link).await?;
        if files.is_empty() {
            tracing::info!(
                job_id = job_id,
                repo = repo,
                cl_link = cl_link,
                "CL overlay has no changed files."
            );
            return Ok(());
        }

        let client = http_client()?;
        let mut applied_paths = Vec::new();
        for file in files {
            let overlay_path = resolve_overlay_relative_path(repo, &file.path)?;
            match file.action.as_str() {
                "new" | "modified" => {
                    download_blob_to_path(&client, &file.sha, &cl_dir.join(&overlay_path)).await?;
                    applied_paths.push(overlay_path.display().to_string());
                }
                "deleted" => {
                    tracing::warn!(
                        job_id = job_id,
                        repo = repo,
                        cl_link = cl_link,
                        path = %overlay_path.display(),
                        "Deleted file in CL is not yet materialized as a whiteout in direct Antares mode; continuing."
                    );
                }
                other => {
                    tracing::warn!(
                        job_id = job_id,
                        repo = repo,
                        cl_link = cl_link,
                        action = other,
                        original_path = %file.path,
                        "Unknown CL action while populating overlay; skipping."
                    );
                }
            }
        }

        tracing::info!(
            job_id = job_id,
            repo = repo,
            cl_link = cl_link,
            cl_dir = %cl_dir.display(),
            applied_file_count = applied_paths.len(),
            applied_files = ?applied_paths,
            "Populated CL overlay directory for direct Antares mount."
        );

        Ok(())
    }

    fn log_tree_recursive(
        root: &Path,
        current: &Path,
        depth: usize,
        max_depth: usize,
        max_entries: usize,
        printed: &mut usize,
    ) {
        if depth >= max_depth || *printed >= max_entries {
            return;
        }

        let entries = match std::fs::read_dir(current) {
            Ok(entries) => entries,
            Err(err) => {
                tracing::warn!("Failed to read {}: {}", current.display(), err);
                return;
            }
        };

        let mut children: Vec<(String, PathBuf, bool)> = Vec::new();
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(err) => {
                    tracing::warn!("read_dir entry error under {}: {}", current.display(), err);
                    continue;
                }
            };

            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            children.push((name, path, is_dir));
        }

        children.sort_by(|a, b| a.0.cmp(&b.0));

        for (_name, path, is_dir) in children {
            if *printed >= max_entries {
                return;
            }

            let rel = path
                .strip_prefix(root)
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| path.display().to_string());
            let indent = "  ".repeat(depth + 1);
            if is_dir {
                tracing::info!("[dicfuse-root] {}{}/", indent, rel);
            } else {
                tracing::info!("[dicfuse-root] {}{}", indent, rel);
            }
            *printed += 1;

            if is_dir {
                log_tree_recursive(root, &path, depth + 1, max_depth, max_entries, printed);
            }
        }
    }

    /// Unmount and cleanup a job overlay filesystem.
    ///
    /// # Arguments
    /// * `job_id` - The job identifier to unmount
    ///
    /// # Returns
    /// The `AntaresConfig` of the unmounted job if it existed.
    #[allow(dead_code)]
    pub async fn unmount_job(job_id: &str) -> Result<Option<AntaresConfig>, DynError> {
        tracing::debug!("Unmounting Antares job: job_id={}", job_id);

        let direct_mount = DIRECT_CL_MOUNTS.lock().await.remove(job_id);
        if let Some(mut direct_mount) = direct_mount {
            run_with_panic_guard(
                format!("Direct CL Antares unmount panicked for job_id={job_id}"),
                direct_mount.fuse.unmount(),
            )
            .await?;
            return Ok(Some(direct_mount.config));
        }

        get_manager()
            .await?
            .umount_job(job_id)
            .await
            .map_err(Into::into)
    }

    /// Best-effort delete of Antares COW layers left after FUSE unmount.
    ///
    /// Scorpio's `umount_job` only tears down the mount; `upper/` and `cl/` dirs
    /// otherwise accumulate across builds on long-lived VMs and fill the disk.
    pub async fn reclaim_overlay_dirs(config: &AntaresConfig) {
        best_effort_remove_dir("upper", &config.upper_dir).await;
        if let Some(cl_dir) = config.cl_dir.as_ref() {
            best_effort_remove_dir("cl", cl_dir).await;
        }
    }

    async fn best_effort_remove_dir(kind: &str, path: &Path) {
        if !path.exists() {
            return;
        }
        match fs::remove_dir_all(path).await {
            Ok(()) => tracing::info!(
                kind = kind,
                path = %path.display(),
                "Reclaimed Antares overlay directory"
            ),
            Err(e) => tracing::warn!(
                kind = kind,
                path = %path.display(),
                error = %e,
                "Failed to reclaim Antares overlay directory"
            ),
        }
    }

    /// When no Antares FUSE mounts are active, delete all `upper/` and `cl/`
    /// children (they are orphans after umount). Returns `(upper_removed, cl_removed)`.
    pub async fn prune_orphan_overlay_dirs() -> Result<(usize, usize), DynError> {
        // Ensure scorpio config / paths are available.
        let _ = get_manager().await?;
        let paths = AntaresPaths::from_global_config();

        // Best-effort: tear down leftover mountpoints under mount_root.
        if let Ok(mut entries) = fs::read_dir(&paths.mount_root).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let mp = entry.path();
                if is_mountpoint(&mp).await {
                    let _ = Command::new("fusermount")
                        .args(["-uz", &mp.to_string_lossy()])
                        .status()
                        .await;
                    let _ = Command::new("umount")
                        .args(["-lf", &mp.to_string_lossy()])
                        .status()
                        .await;
                }
                let _ = fs::remove_dir_all(&mp).await;
            }
        }

        if has_any_mountpoint(&paths.mount_root).await {
            tracing::warn!(
                "Skipping Antares overlay prune: active mountpoints still present under {}",
                paths.mount_root.display()
            );
            return Ok((0, 0));
        }

        let upper_removed = prune_all_child_dirs(&paths.upper_root, "upper").await;
        let cl_removed = prune_all_child_dirs(&paths.cl_root, "cl").await;
        Ok((upper_removed, cl_removed))
    }

    async fn prune_all_child_dirs(root: &Path, kind: &str) -> usize {
        let Ok(mut entries) = fs::read_dir(root).await else {
            return 0;
        };
        let mut removed = 0usize;
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            let is_dir = entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false);
            if !is_dir {
                continue;
            }
            match fs::remove_dir_all(&path).await {
                Ok(()) => {
                    tracing::info!(
                        kind = kind,
                        path = %path.display(),
                        "Pruned orphan Antares overlay directory"
                    );
                    removed += 1;
                }
                Err(e) => tracing::warn!(
                    kind = kind,
                    path = %path.display(),
                    error = %e,
                    "Failed to prune orphan Antares overlay directory"
                ),
            }
        }
        removed
    }

    async fn has_any_mountpoint(root: &Path) -> bool {
        let Ok(mut entries) = fs::read_dir(root).await else {
            return false;
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            if is_mountpoint(&entry.path()).await {
                return true;
            }
        }
        false
    }

    async fn is_mountpoint(path: &Path) -> bool {
        let Ok(output) = Command::new("mountpoint")
            .args(["-q", &path.to_string_lossy()])
            .status()
            .await
        else {
            return false;
        };
        output.success()
    }

    /// Convert panics within scorpiofs calls into regular errors.
    async fn run_with_panic_guard<T, E, F>(context: String, future: F) -> Result<T, DynError>
    where
        F: std::future::Future<Output = Result<T, E>>,
        E: Into<DynError>,
    {
        match AssertUnwindSafe(future).catch_unwind().await {
            Ok(result) => result.map_err(Into::into),
            Err(payload) => Err(Box::new(io_other(format!(
                "{context}: {}",
                panic_payload_to_string(payload.as_ref())
            )))),
        }
    }

    /// Best-effort stringify for panic payloads.
    fn panic_payload_to_string(payload: &(dyn Any + Send)) -> String {
        if let Some(message) = payload.downcast_ref::<&str>() {
            return (*message).to_string();
        }
        if let Some(message) = payload.downcast_ref::<String>() {
            return message.clone();
        }
        "non-string panic payload".to_string()
    }

    #[cfg(test)]
    mod tests {
        use std::io;

        use tempfile::tempdir;

        use super::{
            fusermount_output_indicates_safe_detach, panic_payload_to_string,
            remove_mountpoint_path, resolve_overlay_relative_path, run_with_panic_guard,
        };

        #[tokio::test]
        async fn test_run_with_panic_guard_converts_panic_to_error() {
            let result: Result<(), Box<dyn std::error::Error + Send + Sync>> =
                run_with_panic_guard("panic guard".to_string(), async move {
                    panic!("fuse mount failed");
                    #[allow(unreachable_code)]
                    Ok::<(), io::Error>(())
                })
                .await;

            let err = result.expect_err("panic should be converted into error");
            assert!(err.to_string().contains("panic guard"));
            assert!(err.to_string().contains("fuse mount failed"));
        }

        #[test]
        fn test_panic_payload_to_string_handles_common_payloads() {
            assert_eq!(panic_payload_to_string(&"oops"), "oops");
            assert_eq!(panic_payload_to_string(&"boom".to_string()), "boom");
        }

        #[test]
        fn test_resolve_overlay_relative_path_prefixes_repo_relative_path() {
            let path = resolve_overlay_relative_path("/project/buck2_test", "src/main.rs")
                .expect("path should resolve");
            assert_eq!(path.to_string_lossy(), "project/buck2_test/src/main.rs");
        }

        #[test]
        fn test_resolve_overlay_relative_path_keeps_monorepo_relative_path() {
            let path = resolve_overlay_relative_path(
                "/project/buck2_test",
                "project/buck2_test/toolchains/BUCK",
            )
            .expect("path should resolve");
            assert_eq!(path.to_string_lossy(), "project/buck2_test/toolchains/BUCK");
        }

        #[test]
        fn test_resolve_overlay_relative_path_rejects_escape_sequences() {
            assert!(resolve_overlay_relative_path("/project/buck2_test", "../etc/passwd").is_err());
        }

        #[tokio::test]
        async fn test_remove_mountpoint_path_removes_directory_tree() {
            let tempdir = tempdir().expect("tempdir");
            let mountpoint = tempdir.path().join("mountpoint");
            std::fs::create_dir_all(mountpoint.join("nested")).expect("nested dir");
            std::fs::write(mountpoint.join("nested/file.txt"), b"hello").expect("file");

            remove_mountpoint_path(&mountpoint)
                .await
                .expect("mountpoint cleanup should succeed");

            assert!(!mountpoint.exists());
        }

        #[tokio::test]
        async fn test_remove_mountpoint_path_ignores_missing_path() {
            let tempdir = tempdir().expect("tempdir");
            let mountpoint = tempdir.path().join("missing");

            remove_mountpoint_path(&mountpoint)
                .await
                .expect("missing mountpoint should be ignored");
        }

        #[test]
        fn test_fusermount_output_indicates_safe_detach_matches_known_messages() {
            assert!(fusermount_output_indicates_safe_detach(
                "mountpoint not mounted"
            ));
            assert!(fusermount_output_indicates_safe_detach("Invalid argument"));
            assert!(fusermount_output_indicates_safe_detach(
                "not found in /etc/mtab"
            ));
            assert!(!fusermount_output_indicates_safe_detach(
                "permission denied"
            ));
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    //! Stub implementation for non-Linux platforms.

    use std::{error::Error, path::PathBuf};

    type DynError = Box<dyn Error + Send + Sync>;

    #[derive(Debug, Clone)]
    pub struct AntaresConfig {
        pub mountpoint: PathBuf,
        pub job_id: String,
        pub upper_dir: PathBuf,
        pub cl_dir: Option<PathBuf>,
    }

    /// Mounting Antares requires `scorpiofs` (Linux-only in this repository).
    pub async fn mount_job(
        _job_id: &str,
        _repo: &str,
        _cl: Option<&str>,
    ) -> Result<AntaresConfig, DynError> {
        Err(Box::new(std::io::Error::other(
            "Antares/scorpiofs is only supported on Linux",
        )))
    }

    /// Unmounting Antares requires `scorpiofs` (Linux-only in this repository).
    #[allow(dead_code)]
    pub async fn unmount_job(_job_id: &str) -> Result<Option<AntaresConfig>, DynError> {
        Err(Box::new(std::io::Error::other(
            "Antares/scorpiofs is only supported on Linux",
        )))
    }

    #[allow(dead_code)]
    pub async fn reclaim_overlay_dirs(_config: &AntaresConfig) {}

    #[allow(dead_code)]
    pub async fn prune_orphan_overlay_dirs() -> Result<(usize, usize), DynError> {
        Ok((0, 0))
    }

    #[allow(dead_code)]
    pub(crate) async fn warmup_dicfuse() -> Result<(), DynError> {
        Ok(())
    }
}

pub use imp::*;
