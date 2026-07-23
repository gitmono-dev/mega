use std::{
    collections::{HashMap, HashSet},
    error::Error,
    io::BufReader,
    path::{Path, PathBuf},
    process::{ExitStatus, Stdio},
};

use anyhow::anyhow;
use api_model::buck2::{
    status::Status,
    types::{ProjectRelativePath, TaskPhase},
    ws::{WSBuildContext, WSMessage, WSTargetBuildStatusEvent},
};
use common::config::BuildConfig;
use once_cell::sync::Lazy;
use td_util::{command::spawn, file_io::file_writer, file_tail::tail_compressed_buck2_events};
use td_util_buck::{
    cells::CellInfo,
    discovery_scope::{
        compute_discovery_scope, detect_subproject_buck_root, strip_subproject_changes,
    },
    owners::Owners,
    platform::{append_platform_config, platform_config_flags},
    run::{Buck2, targets_arguments},
    target_status::{BuildState, EVENT_LOG_FILE, Event, LogicalActionId, TargetBuildStatusUpdate},
    targets::{BuckTarget, Targets},
    types::{RuleType, TargetLabel},
};
use tokio::{
    io::AsyncBufReadExt,
    process::Command,
    sync::mpsc::{self, UnboundedSender},
    task::JoinHandle,
    time::{Duration, interval},
};
use tokio_util::sync::CancellationToken;

// Import complete Error trait for better error handling
use crate::repo::changes::Changes;
use crate::repo::diff;

const MAX_BATCH_SIZE: usize = 100;
const FLUSH_INTERVAL_MS: u64 = 100;

#[allow(dead_code)]
static PROJECT_ROOT: Lazy<String> =
    Lazy::new(|| std::env::var("BUCK_PROJECT_ROOT").expect("BUCK_PROJECT_ROOT must be set"));

const DEFAULT_PREHEAT_SHALLOW_DEPTH: usize = 3;
static BUILD_CONFIG: Lazy<Option<BuildConfig>> = Lazy::new(load_build_config);

#[derive(Debug, Clone)]
struct AntaresMountPair {
    old_mount_id: String,
    old_mount_point: String,
    new_mount_id: String,
    new_mount_point: String,
    old_unmounted: bool,
}

/// Enable buck2 remote cache when set to `1`, `true`, `yes`, or `on`.
///
/// Default: disabled (`--no-remote-cache`) so incremental builds always compile
/// locally and catch syntax errors immediately.
fn buck_remote_cache_enabled() -> bool {
    match std::env::var("ORION_BUCK_REMOTE_CACHE") {
        Ok(value) => {
            let normalized = value.trim().to_ascii_lowercase();
            matches!(normalized.as_str(), "1" | "true" | "yes" | "on")
        }
        Err(_) => false,
    }
}

fn retain_antares_mounts() -> bool {
    match std::env::var("ORION_RETAIN_ANTARES_MOUNTS") {
        Ok(value) => {
            let normalized = value.trim().to_ascii_lowercase();
            !(normalized.is_empty()
                || normalized == "0"
                || normalized == "false"
                || normalized == "no"
                || normalized == "off")
        }
        Err(_) => false,
    }
}

fn antares_unmount_grace_duration() -> Duration {
    const DEFAULT_MS: u64 = 150;
    match std::env::var("ORION_ANTARES_UNMOUNT_GRACE_MS") {
        Ok(raw) => match raw.trim().parse::<u64>() {
            Ok(ms) => Duration::from_millis(ms.clamp(0, 3_000)),
            Err(_) => {
                tracing::warn!(
                    value = %raw,
                    default_ms = DEFAULT_MS,
                    "invalid ORION_ANTARES_UNMOUNT_GRACE_MS, using default"
                );
                Duration::from_millis(DEFAULT_MS)
            }
        },
        Err(_) => Duration::from_millis(DEFAULT_MS),
    }
}

async fn cleanup_antares_mount(
    task_id: &str,
    mount_id: &str,
    mount_point: Option<&str>,
    reason: &str,
) {
    let grace = antares_unmount_grace_duration();
    if !grace.is_zero() {
        tracing::info!(
            "[Task {}] Waiting {:?} before Antares mount cleanup (mount_id={}, mountpoint={}, reason={})",
            task_id,
            grace,
            mount_id,
            mount_point.unwrap_or("<unknown>"),
            reason,
        );
        tokio::time::sleep(grace).await;
    }

    match unmount_antares_fs(mount_id).await {
        Ok(true) => tracing::info!(
            "[Task {}] Cleaned Antares mount (mount_id={}, mountpoint={}, reason={})",
            task_id,
            mount_id,
            mount_point.unwrap_or("<unknown>"),
            reason,
        ),
        Ok(false) => tracing::warn!(
            "[Task {}] Antares mount cleanup reported no-op (mount_id={}, mountpoint={}, reason={})",
            task_id,
            mount_id,
            mount_point.unwrap_or("<unknown>"),
            reason,
        ),
        Err(err) => tracing::warn!(
            "[Task {}] Failed to cleanup Antares mount (mount_id={}, mountpoint={}, reason={}): {}",
            task_id,
            mount_id,
            mount_point.unwrap_or("<unknown>"),
            reason,
            err,
        ),
    }
}

async fn cleanup_antares_mount_pair(task_id: &str, mounts: &AntaresMountPair, reason: &str) {
    if !mounts.old_unmounted {
        cleanup_antares_mount(
            task_id,
            &mounts.old_mount_id,
            Some(&mounts.old_mount_point),
            reason,
        )
        .await;
    }
    cleanup_antares_mount(
        task_id,
        &mounts.new_mount_id,
        Some(&mounts.new_mount_point),
        reason,
    )
    .await;
}

/// The old-repo mount is only needed for target discovery; release it before buck2 build.
async fn unmount_discovery_old_mount(task_id: &str, mounts: &mut AntaresMountPair) {
    if mounts.old_unmounted {
        return;
    }

    tracing::info!(
        "[Task {}] Unmounting old-repo Antares view after target discovery (mount_id={}, mountpoint={})",
        task_id,
        mounts.old_mount_id,
        mounts.old_mount_point,
    );
    cleanup_antares_mount(
        task_id,
        &mounts.old_mount_id,
        Some(&mounts.old_mount_point),
        "old-repo mount no longer needed after target discovery",
    )
    .await;
    mounts.old_unmounted = true;
}

/// Mount an Antares overlay filesystem for a build job.
///
/// Creates a new Antares overlay mount using scorpiofs. The underlying Dicfuse
/// layer provides read-only access to the repository, while the overlay allows
/// copy-on-write modifications during the build.
///
/// # Arguments
/// * `job_id` - Unique identifier for this build job
/// * `cl` - Optional changelist layer name
///
/// # Returns
/// Returns a tuple `(mountpoint, mount_id)` on success.
pub async fn mount_antares_fs(
    job_id: &str,
    repo: &str,
    cl: Option<&str>,
) -> Result<(String, String), Box<dyn std::error::Error + Send + Sync>> {
    tracing::debug!(
        "Preparing to mount Antares FS: job_id={}, repo={}, cl={:?}",
        job_id,
        repo,
        cl
    );

    let config = crate::antares::mount_job(job_id, repo, cl).await?;

    let mountpoint = config.mountpoint.to_string_lossy().to_string();
    let mount_id = config.job_id.clone();

    tracing::info!(
        "Antares mount created successfully: mountpoint={}, mount_id={}",
        mountpoint,
        mount_id
    );

    Ok((mountpoint, mount_id))
}

/// Unmount an Antares overlay filesystem and reclaim COW dirs unless retain is set.
///
/// # Arguments
/// * `mount_id` - The job/mount ID to unmount
///
/// # Returns
/// * `Ok(true)` - Unmount succeeded (dirs reclaimed when not retaining)
/// * `Ok(false)` - No mount was found for `mount_id`
/// * `Err` - Unmount failed
#[allow(dead_code)]
pub async fn unmount_antares_fs(mount_id: &str) -> Result<bool, Box<dyn Error + Send + Sync>> {
    tracing::info!("Starting unmount for mount_id: {}", mount_id);

    let config = crate::antares::unmount_job(mount_id).await?;
    let Some(config) = config else {
        tracing::warn!("Unmount reported no-op for mount_id: {}", mount_id);
        return Ok(false);
    };

    if retain_antares_mounts() {
        tracing::info!(
            "ORION_RETAIN_ANTARES_MOUNTS set — keeping overlay dirs for mount_id={} upper={} cl={:?}",
            mount_id,
            config.upper_dir.display(),
            config.cl_dir.as_ref().map(|p| p.display().to_string())
        );
    } else {
        crate::antares::reclaim_overlay_dirs(&config).await;
    }

    tracing::info!("Unmount succeeded for mount_id: {}", mount_id);
    Ok(true)
}

/// Pre-warm FUSE metadata by walking the directory tree.
///
/// After the Antares `wait_for_mount_ready` integration, the heavy deep preload
/// is performed by the scorpio daemon itself. This function serves as a secondary
/// fallback to catch any directories that might have been missed or expired.
///
/// We use a shallow Rust-native walk (instead of `ls -lR`) for better control
/// and error resilience.
#[allow(dead_code)]
fn preheat(repo_path: &Path) -> anyhow::Result<()> {
    tracing::info!(repo = ?repo_path, "preheat: starting lightweight metadata warmup");
    let start = std::time::Instant::now();
    let depth = preheat_shallow_depth();
    preheat_shallow(repo_path, depth)?;
    tracing::info!(
        repo = ?repo_path,
        elapsed_ms = start.elapsed().as_millis(),
        depth = depth,
        "preheat: completed"
    );
    Ok(())
}

/// Preheat a shallow directory tree to reduce cold-start metadata misses.
fn preheat_shallow(repo_path: &Path, max_depth: usize) -> anyhow::Result<()> {
    if max_depth == 0 {
        return Ok(());
    }

    let mut stack = vec![(repo_path.to_path_buf(), 0usize)];
    while let Some((path, depth)) = stack.pop() {
        let entries = match std::fs::read_dir(&path) {
            Ok(entries) => entries,
            Err(err) => {
                tracing::warn!("Preheat shallow read_dir failed for {path:?}: {err}");
                continue;
            }
        };

        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(err) => {
                    tracing::warn!("Preheat shallow entry error under {path:?}: {err}");
                    continue;
                }
            };

            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(err) => {
                    tracing::warn!(
                        "Preheat shallow file_type failed for {:?}: {err}",
                        entry.path()
                    );
                    continue;
                }
            };

            // Touch metadata to warm FUSE cache.
            let _ = entry.metadata();

            if file_type.is_dir() && depth < max_depth {
                stack.push((entry.path(), depth + 1));
            }
        }
    }

    Ok(())
}

fn preheat_shallow_depth() -> usize {
    if let Some(depth) = parse_env_usize("ORION_PREHEAT_SHALLOW_DEPTH") {
        return depth;
    }

    if let Some(depth) = parse_env_usize("MEGA_BUILD__ORION_PREHEAT_SHALLOW_DEPTH") {
        return depth;
    }

    build_config()
        .map(|config| config.orion_preheat_shallow_depth)
        .unwrap_or(DEFAULT_PREHEAT_SHALLOW_DEPTH)
}

fn parse_env_usize(key: &str) -> Option<usize> {
    match std::env::var(key) {
        Ok(value) => match value.parse::<usize>() {
            Ok(depth) => Some(depth),
            Err(_) => {
                tracing::warn!("Invalid {key}={value:?}, ignoring.");
                None
            }
        },
        Err(_) => None,
    }
}

fn build_config() -> Option<&'static BuildConfig> {
    BUILD_CONFIG.as_ref()
}

fn load_build_config() -> Option<BuildConfig> {
    let path = resolve_config_path()?;
    let path_str = path.to_str()?;
    match common::config::Config::new(path_str) {
        Ok(config) => Some(config.build),
        Err(err) => {
            tracing::warn!("Failed to load MEGA config from {path:?}: {err}");
            None
        }
    }
}

fn resolve_config_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("MEGA_CONFIG") {
        let path = PathBuf::from(path);
        if path.exists() {
            return Some(path);
        }
        tracing::warn!("MEGA_CONFIG points to missing file: {path:?}");
    }

    if let Ok(cwd) = std::env::current_dir() {
        let path = cwd.join("config/config.toml");
        if path.exists() {
            return Some(path);
        }
    }

    if let Ok(base_dir) = std::env::var("MEGA_BASE_DIR") {
        let path = PathBuf::from(base_dir).join("etc/config.toml");
        if path.exists() {
            return Some(path);
        }
    }

    None
}

/// Get target of a specific repo under tmp directory.
///
/// Note: `preheat()` was previously called here to warm the kernel VFS/FUSE
/// cache, but it has been removed. The new `warmup_dicfuse()` at startup
/// populates the Dicfuse backing store which makes subsequent FUSE reads
/// fast, and `preheat_shallow()` in `get_build_targets()` handles the
/// per-mount VFS cache warming.
fn all_changes_are_added(changes: &[Status<ProjectRelativePath>]) -> bool {
    !changes.is_empty()
        && changes
            .iter()
            .all(|change| matches!(change, Status::Added(_)))
}

/// Subproject import (e.g. all-added `rk8s/`): only consider paths under `project/`.
fn filter_changes_under_prefix(
    changes: &[Status<ProjectRelativePath>],
    prefix: &str,
) -> Vec<Status<ProjectRelativePath>> {
    let prefix_slash = format!("{prefix}/");
    changes
        .iter()
        .filter(|change| {
            let path = change.get().as_str();
            path == prefix || path.starts_with(&prefix_slash)
        })
        .cloned()
        .collect()
}

/// Paths suitable for `buck2 uquery owner()` when seeding all-added subproject builds.
/// Skips package manifests and vendored trees so we do not select every crate in `project/`.
fn is_owner_seed_path(path: &str) -> bool {
    if path.is_empty() {
        return false;
    }
    let file_name = path.rsplit('/').next().unwrap_or(path);
    if matches!(
        file_name,
        "BUCK" | "TARGETS" | "BUCK.v2" | "Cargo.toml" | "Cargo.lock" | ".buckconfig"
    ) {
        return false;
    }
    !(path.contains("/vendor/") || path.ends_with("/vendor"))
}

fn filter_owner_seed_changes(
    changes: &[Status<ProjectRelativePath>],
) -> Vec<Status<ProjectRelativePath>> {
    changes
        .iter()
        .filter(|change| is_owner_seed_path(change.get().as_str()))
        .cloned()
        .collect()
}

/// Paths passed to `owner()` when the CL is all-added.
///
/// All-added sets use an empty base graph (old-repo `buck2 targets` is skipped).
/// Running graph diff with `EmptyBasePolicy::SelectAll` would mark every target in
/// the diff graph as impacted; seed from changed source paths instead.
fn owner_seed_changes_for_discovery(
    all_added_subproject: bool,
    all_added: bool,
    changes: &[Status<ProjectRelativePath>],
) -> Vec<Status<ProjectRelativePath>> {
    match (all_added_subproject, all_added) {
        (true, _) => {
            let under_project =
                filter_changes_under_prefix(changes, ALL_ADDED_SUBPROJECT_BUILD_PREFIX);
            filter_owner_seed_changes(&under_project)
        }
        (false, true) => filter_owner_seed_changes(changes),
        (false, false) => changes.to_vec(),
    }
}

const ALL_ADDED_SUBPROJECT_BUILD_PREFIX: &str = "project";

fn get_repo_targets(
    file_name: &str,
    repo_path: &Path,
    cells: Option<&CellInfo>,
    query_patterns: Option<&[String]>,
) -> anyhow::Result<Targets> {
    const MAX_ATTEMPTS: usize = 2;
    let jsonl_path = PathBuf::from(repo_path).join(file_name);

    for attempt in 1..=MAX_ATTEMPTS {
        tracing::debug!("Get targets for repo {repo_path:?} (attempt {attempt}/{MAX_ATTEMPTS})");

        // DEBUG: Log exact command execution context
        tracing::debug!(
            repo_path = %repo_path.display(),
            cwd_exists = repo_path.exists(),
            "DEBUG: Buck2 command execution context"
        );

        let mut command = std::process::Command::new("buck2");
        command
            .env("BUCKD_STARTUP_TIMEOUT", "30")
            .env("BUCKD_STARTUP_INIT_TIMEOUT", "1200");

        // Add base targets arguments
        command.args(targets_arguments());
        append_platform_config(&mut command);

        // If cells info is provided, query all cells; otherwise just query root cell
        if let Some(cells_info) = cells {
            let cell_patterns = match query_patterns {
                Some(patterns) if !patterns.is_empty() => patterns.to_vec(),
                _ => cells_info.get_all_cell_patterns(repo_path),
            };
            tracing::debug!("Querying targets for cells: {:?}", cell_patterns);
            command.args(&cell_patterns);
        } else {
            // Default: only query root cell
            command.arg("//...");
        }

        // DEBUG: Log the current directory
        tracing::debug!(current_dir = %repo_path.display(), "DEBUG: Setting buck2 current_dir");
        command.current_dir(repo_path);

        // DEBUG: Check if we can stat the required files
        let buckconfig_path = repo_path.join(".buckconfig");
        let buck_path = repo_path.join("BUCK");
        tracing::debug!(
            current_dir = %repo_path.display(),
            buckconfig_exists = buckconfig_path.exists(),
            buck_exists = buck_path.exists(),
            "DEBUG: Cell marker files check"
        );

        command.stderr(Stdio::piped());
        let (mut child, stdout) = spawn(command)?;
        let mut stderr = child.stderr.take().expect("stderr should be piped");

        // Consume stderr in background thread to prevent pipe deadlock
        let stderr_handle = std::thread::spawn(move || {
            let mut buf = Vec::new();
            std::io::Read::read_to_end(&mut stderr, &mut buf)
                .map(|_| String::from_utf8_lossy(&buf).to_string())
                .unwrap_or_else(|_| "<failed to read stderr>".to_string())
        });

        let mut writer = file_writer(&jsonl_path)?;
        std::io::copy(&mut BufReader::new(stdout), &mut writer)
            .map_err(|err| anyhow!("Failed to copy output to stdout: {}", err))?;
        writer
            .flush()
            .map_err(|err| anyhow!("Failed to flush writer: {}", err))?;

        let status = child.wait()?;
        let stderr_output = stderr_handle
            .join()
            .unwrap_or_else(|_| "<stderr thread panicked>".to_string());

        if status.success() {
            if !stderr_output.trim().is_empty() {
                tracing::debug!(
                    "buck2 targets succeeded for repo {:?}, stderr: {}",
                    repo_path,
                    stderr_output.trim()
                );
            }
            return Targets::from_file(&jsonl_path);
        }

        tracing::warn!(
            "buck2 targets failed with status {} for repo {:?}. stderr: {}",
            status,
            repo_path,
            stderr_output.trim()
        );
        if attempt < MAX_ATTEMPTS {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }

    Err(anyhow!(
        "buck2 targets failed after {MAX_ATTEMPTS} attempts for repo {:?}",
        repo_path
    ))
}

fn fallback_targets_for_errored_packages(
    base: &Targets,
    diff: &Targets,
    changes: &Changes,
) -> Vec<TargetLabel> {
    let error_count = diff.errors().count();
    if error_count == 0 {
        return Vec::new();
    }

    let error_packages: HashSet<_> = diff.errors().map(|error| error.package.clone()).collect();
    let mut seen = HashSet::new();
    let mut fallback = Vec::new();

    for target in base.targets() {
        if !error_packages.contains(&target.package) || !changes.contains_package(&target.package) {
            continue;
        }

        let label = target.label();
        if seen.insert(label.clone()) {
            fallback.push(label);
        }
    }

    if fallback.is_empty() {
        tracing::warn!(
            error_count,
            errored_packages = error_packages.len(),
            "buck2 targets returned package errors but no fallback targets could be recovered from the base graph."
        );
    } else {
        tracing::warn!(
            error_count,
            errored_packages = error_packages.len(),
            fallback_targets = fallback.len(),
            "Recovered impacted targets from the base graph for packages that failed to parse during buck2 targets."
        );
    }

    fallback
}

const OWNER_QUERY_BATCH_SIZE: usize = 500;

/// When the `buck2 targets` graph has no impacted nodes (e.g. all-added CL with only
/// package errors + imports in jsonl), resolve owners directly via `buck2 uquery owner()`.
fn fallback_targets_from_owners(
    buck2: &mut Buck2,
    changes: &[Status<ProjectRelativePath>],
) -> anyhow::Result<Vec<TargetLabel>> {
    let paths: Vec<ProjectRelativePath> = changes
        .iter()
        .map(|change| change.get().clone())
        .filter(|path| !path.as_str().is_empty())
        .collect();
    if paths.is_empty() {
        return Ok(Vec::new());
    }

    let mut seen = HashSet::new();
    let mut targets = Vec::new();
    let mut batches = 0usize;

    for chunk in paths.chunks(OWNER_QUERY_BATCH_SIZE) {
        batches += 1;
        let json = match buck2.owners(&[], chunk) {
            Ok(json) => json,
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    owner_batch = batches,
                    "buck2 uquery owner() batch failed; continuing with remaining paths."
                );
                continue;
            }
        };
        let owners = Owners::from_json(&json)?;
        for label in owners.all_targets() {
            if label_is_toolchain_or_platform(label) {
                continue;
            }
            if seen.insert(label.clone()) {
                targets.push(label.clone());
            }
        }
    }

    if targets.is_empty() {
        tracing::warn!(
            change_paths = paths.len(),
            owner_batches = batches,
            "buck2 uquery owner() returned no build targets for the changed paths."
        );
    } else {
        tracing::info!(
            change_paths = paths.len(),
            owner_batches = batches,
            recovered_targets = targets.len(),
            "Recovered impacted Buck targets via buck2 uquery owner()."
        );
    }

    Ok(targets)
}

/// Rule types that define toolchains, platforms, or configuration nodes.
///
/// These are build-system plumbing, not business targets. We never want to
/// propagate impact *through* them (a pure-Rust change must not fan out into
/// JVM/Android/CXX toolchain helpers), nor select them as explicit `buck2
/// build` targets (e.g. `jdk_system_image`, `__android_sdk_tools__`).
fn is_toolchain_or_platform_rule(rule_type: &RuleType) -> bool {
    let short = rule_type.short();

    const EXACT: &[&str] = &[
        "platform",
        "execution_platform",
        "execution_platforms",
        "constraint_setting",
        "constraint_value",
        "config_setting",
        "configuration",
        "configured_alias",
        "toolchain_alias",
    ];

    EXACT.contains(&short)
        || short == "toolchain"
        || short.ends_with("_toolchain")
        || short.starts_with("toolchain_")
}

/// Whether a package (cell-qualified, e.g. `root//project/buck2_test/toolchains`)
/// is a toolchain or platform definition package.
fn package_is_toolchain_or_platform(package: &str) -> bool {
    let (cell, rel) = package.split_once("//").unwrap_or(("", package));

    if cell == "toolchains" {
        return true;
    }

    for plumbing in ["toolchains", "platforms"] {
        if rel == plumbing
            || rel.starts_with(&format!("{plumbing}/"))
            || rel.ends_with(&format!("/{plumbing}"))
            || rel.contains(&format!("/{plumbing}/"))
        {
            return true;
        }
    }

    false
}

/// Whether a concrete target is a toolchain/platform helper that should be
/// excluded from the explicit build set.
fn is_toolchain_or_platform_target(target: &BuckTarget) -> bool {
    is_toolchain_or_platform_rule(&target.rule_type)
        || package_is_toolchain_or_platform(target.package.as_str())
        || matches!(
            target.name.as_str(),
            "jdk_system_image" | "__android_sdk_tools__"
        )
}

/// Heuristic for fallback labels (we only have the label string, not the
/// `BuckTarget`), e.g. `root//project/buck2_test/toolchains:jdk_system_image`.
fn label_is_toolchain_or_platform(label: &TargetLabel) -> bool {
    let label = label.as_str();
    let package = label.rsplit_once(':').map(|(pkg, _)| pkg).unwrap_or(label);
    let name = label.rsplit_once(':').map(|(_, name)| name).unwrap_or("");

    package_is_toolchain_or_platform(package)
        || matches!(name, "jdk_system_image" | "__android_sdk_tools__")
}

fn is_rust_build_rule(rule_type: &RuleType) -> bool {
    matches!(
        rule_type.short(),
        "rust_library" | "rust_binary" | "rust_test"
    )
}

/// Buckal-generated helper targets that must not be passed to `buck2 build`.
fn label_is_buckal_plumbing_name(name: &str) -> bool {
    matches!(name, "vendor" | "manifest") || name.starts_with("build-script")
}

fn is_buckal_plumbing_target(target: &BuckTarget) -> bool {
    label_is_buckal_plumbing_name(target.name.as_str())
        || matches!(
            target.rule_type.short(),
            "cargo_manifest" | "filegroup" | "buildscript_run"
        )
}

fn target_package_under_prefix(package: &str, package_prefix: &str) -> bool {
    let qualified = format!("root//{package_prefix}");
    let qualified_slash = format!("{qualified}/");
    package == qualified || package.starts_with(&qualified_slash)
}

fn rust_build_targets_in_package<'a>(diff: &'a Targets, package: &str) -> Vec<&'a BuckTarget> {
    diff.targets()
        .filter(|t| t.package.as_str() == package && is_rust_build_rule(&t.rule_type))
        .collect()
}

/// `buck2 uquery owner()` on buckal trees often returns `:vendor` filegroups.
/// Map those to the real `rust_library` / `rust_binary` targets in the same package.
fn normalize_owner_targets_to_rust(diff: &Targets, seeds: Vec<TargetLabel>) -> Vec<TargetLabel> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();

    for label in seeds {
        if label_is_toolchain_or_platform(&label) {
            continue;
        }
        let label_str = label.as_str();
        let package = label_str
            .rsplit_once(':')
            .map(|(pkg, _)| pkg)
            .unwrap_or(label_str);
        let name = label_str.rsplit_once(':').map(|(_, n)| n).unwrap_or("");

        let push_rust_in_package = |out: &mut Vec<_>, seen: &mut HashSet<_>| {
            for target in rust_build_targets_in_package(diff, package) {
                let rust_label = target.label();
                if seen.insert(rust_label.clone()) {
                    out.push(rust_label);
                }
            }
        };

        if label_is_buckal_plumbing_name(name) {
            push_rust_in_package(&mut out, &mut seen);
            continue;
        }

        if let Some(target) = diff.targets().find(|t| t.label() == label)
            && (is_buckal_plumbing_target(target) || !is_rust_build_rule(&target.rule_type))
        {
            push_rust_in_package(&mut out, &mut seen);
            continue;
        }

        if seen.insert(label.clone()) {
            out.push(label);
        }
    }

    out
}

fn package_prefix_from_universe_patterns(patterns: &[String]) -> Option<String> {
    for pattern in patterns {
        let stripped = pattern.strip_prefix("root//")?;
        let prefix = stripped.strip_suffix("/...")?;
        if prefix.is_empty() || prefix.contains("...") {
            continue;
        }
        return Some(prefix.to_owned());
    }
    None
}

fn collect_impacted_targets(
    base: &Targets,
    diff: &Targets,
    changes: &Changes,
    empty_base_policy: diff::EmptyBasePolicy,
) -> Vec<TargetLabel> {
    let immediate =
        diff::immediate_target_changes_with_policy(base, diff, changes, false, empty_base_policy);
    // Do not propagate impact *through* toolchain/platform/config nodes: a
    // change to (or near) a toolchain definition must not drag in every target
    // that resolves that toolchain.
    let recursive = diff::recursive_target_changes(diff, changes, &immediate, None, |rule_type| {
        !is_toolchain_or_platform_rule(rule_type)
    });

    let mut excluded_helpers = 0usize;
    let mut targets: Vec<_> = recursive
        .into_iter()
        .flatten()
        .filter(|(target, _)| {
            // Never build toolchain/platform helpers as explicit targets; if a
            // real target needs them, buck2 still builds them transitively.
            let keep = !is_toolchain_or_platform_target(target);
            if !keep {
                excluded_helpers += 1;
            }
            keep
        })
        .map(|(target, _)| target.label())
        .collect();
    let mut seen: HashSet<_> = targets.iter().cloned().collect();

    for label in fallback_targets_for_errored_packages(base, diff, changes) {
        if label_is_toolchain_or_platform(&label) {
            excluded_helpers += 1;
            continue;
        }
        if seen.insert(label.clone()) {
            targets.push(label);
        }
    }

    if excluded_helpers > 0 {
        tracing::info!(
            excluded_helpers,
            "Excluded toolchain/platform helper targets from the build set."
        );
    }

    if targets.is_empty() {
        tracing::info!(
            changes_count = changes.cell_paths().count(),
            base_targets = base.len_targets_upperbound(),
            diff_targets = diff.len_targets_upperbound(),
            diff_errors = diff.errors().count(),
            "No impacted targets found. Changes may not match any target inputs or packages."
        );
    } else {
        tracing::info!(
            impacted_targets = targets.len(),
            diff_errors = diff.errors().count(),
            "Found impacted targets"
        );
    }

    targets
}

/// Expand impacted targets with reverse-deps from the full cell universe (scheme D).
fn expand_impacted_with_rdeps(
    buck2: &mut Buck2,
    seeds: &[TargetLabel],
    universe_patterns: &[String],
) -> anyhow::Result<Vec<TargetLabel>> {
    if seeds.is_empty() {
        return Ok(Vec::new());
    }

    let rdeps = buck2.uquery_rdeps(seeds, universe_patterns)?;
    let mut seen: HashSet<TargetLabel> = seeds.iter().cloned().collect();
    let mut expanded = seeds.to_vec();

    for label in rdeps {
        if label_is_toolchain_or_platform(&label) {
            continue;
        }
        if seen.insert(label.clone()) {
            expanded.push(label);
        }
    }

    Ok(expanded)
}

/// Reverse-deps expansion using the in-memory `buck2 targets` graph.
///
/// Avoids `buck2 uquery rdeps`, which can fail when a dependency references a
/// missing toolchain cell (e.g. `toolchains//:cxx_no_default_deps`).
fn expand_impacted_with_graph_rdeps(
    diff: &Targets,
    changes: &Changes,
    seeds: &[TargetLabel],
    package_prefix: &str,
) -> Vec<TargetLabel> {
    if seeds.is_empty() {
        return Vec::new();
    }

    let target_by_label: HashMap<TargetLabel, &BuckTarget> =
        diff.targets().map(|t| (t.label(), t)).collect();

    let mut recursive_seeds = Vec::new();
    for label in seeds {
        let Some(target) = target_by_label.get(label) else {
            continue;
        };
        if !target_package_under_prefix(target.package.as_str(), package_prefix) {
            continue;
        }
        recursive_seeds.push((
            *target,
            diff::ImpactTraceData::new(target, diff::RootImpactKind::Inputs),
        ));
    }

    if recursive_seeds.is_empty() {
        return seeds.to_vec();
    }

    let impact = diff::GraphImpact::from_recursive(recursive_seeds);
    let layers = diff::recursive_target_changes(diff, changes, &impact, None, |rule_type| {
        !is_toolchain_or_platform_rule(rule_type)
    });

    let mut seen: HashSet<TargetLabel> = seeds.iter().cloned().collect();
    let mut expanded = seeds.to_vec();

    for layer in layers {
        for (target, _) in layer {
            if !target_package_under_prefix(target.package.as_str(), package_prefix) {
                continue;
            }
            if is_toolchain_or_platform_target(target) || is_buckal_plumbing_target(target) {
                continue;
            }
            if !is_rust_build_rule(&target.rule_type) {
                continue;
            }
            let label = target.label();
            if seen.insert(label.clone()) {
                expanded.push(label);
            }
        }
    }

    expanded
}

fn maybe_expand_narrow_targets(
    buck2: &mut Buck2,
    diff: &Targets,
    changes: &Changes,
    targets: Vec<TargetLabel>,
    narrow: bool,
    universe_patterns: &[String],
    graph_rdeps_prefix: Option<&str>,
) -> Vec<TargetLabel> {
    if !narrow || targets.is_empty() {
        return targets;
    }

    if let Some(prefix) = graph_rdeps_prefix {
        let expanded = expand_impacted_with_graph_rdeps(diff, changes, &targets, prefix);
        if expanded.len() > targets.len() {
            tracing::info!(
                seeds = targets.len(),
                after_rdeps = expanded.len(),
                package_prefix = prefix,
                "Expanded narrowed discovery seeds with in-graph rdeps."
            );
        }
        return expanded;
    }

    match expand_impacted_with_rdeps(buck2, &targets, universe_patterns) {
        Ok(expanded) => {
            if expanded.len() > targets.len() {
                tracing::info!(
                    seeds = targets.len(),
                    after_rdeps = expanded.len(),
                    "Expanded narrowed discovery seeds with buck2 uquery rdeps."
                );
            }
            expanded
        }
        Err(err) => {
            if let Some(prefix) = package_prefix_from_universe_patterns(universe_patterns) {
                tracing::warn!(
                    error = %err,
                    seed_count = targets.len(),
                    package_prefix = %prefix,
                    "buck2 uquery rdeps failed; falling back to in-graph rdeps."
                );
                let expanded = expand_impacted_with_graph_rdeps(diff, changes, &targets, &prefix);
                if expanded.len() > targets.len() {
                    tracing::info!(
                        seeds = targets.len(),
                        after_rdeps = expanded.len(),
                        "Expanded narrowed discovery seeds with in-graph rdeps fallback."
                    );
                }
                expanded
            } else {
                tracing::warn!(
                    error = %err,
                    seed_count = targets.len(),
                    "buck2 uquery rdeps failed; keeping narrowed impacted targets only."
                );
                targets
            }
        }
    }
}

fn has_path_component_suffix(candidate: &str, suffix: &str) -> bool {
    candidate == suffix
        || candidate
            .strip_suffix(suffix)
            .is_some_and(|prefix| prefix.ends_with('/'))
}

fn remap_repo_local_change_paths(
    project_root: &Path,
    diff: &Targets,
    changes: &[Status<ProjectRelativePath>],
) -> (Vec<Status<ProjectRelativePath>>, usize) {
    let mut remapped_count = 0usize;
    let mut normalized_changes = Vec::with_capacity(changes.len());

    for change in changes {
        let original = change.get().as_str();
        if original.is_empty() || project_root.join(original).exists() {
            normalized_changes.push(change.clone());
            continue;
        }

        let mut candidates: HashSet<String> = HashSet::new();
        for target in diff.targets() {
            for input in target.inputs.iter() {
                let candidate_path = input.path();
                let candidate = candidate_path.as_str();
                if !has_path_component_suffix(candidate, original) {
                    continue;
                }
                if project_root.join(candidate).exists() {
                    candidates.insert(candidate.to_owned());
                    if candidates.len() > 1 {
                        break;
                    }
                }
            }
            if candidates.len() > 1 {
                break;
            }
        }

        if candidates.len() == 1 {
            let remapped = candidates.into_iter().next().expect("single candidate");
            tracing::info!(
                original_path = %original,
                remapped_path = %remapped,
                "Remapping unresolved repo-local change path to a unique Buck input path."
            );
            remapped_count += 1;
            normalized_changes.push(
                change
                    .clone()
                    .into_map(|_| ProjectRelativePath::new(&remapped)),
            );
        } else {
            normalized_changes.push(change.clone());
        }
    }

    (normalized_changes, remapped_count)
}

/// Run buck2-change-detector to get targets to build.
///
/// # Note
/// `mount_point` must be a mounted repository or CL path.
/// `mega_changes` follows the hybrid path contract used end-to-end:
/// repo-local files are repo-relative, while shared external paths stay
/// monorepo-relative.
async fn get_build_targets(
    old_repo_mount_point: &str,
    mount_point: &str,
    mega_changes: Vec<Status<ProjectRelativePath>>,
) -> anyhow::Result<Vec<TargetLabel>> {
    tracing::info!("Get cells at {:?}", mount_point);
    let mount_path = PathBuf::from(mount_point);
    let old_repo = PathBuf::from(old_repo_mount_point);

    // DEBUG: Log path details before Buck2 operations
    tracing::debug!(
        mount_point = %mount_point,
        mount_path = %mount_path.display(),
        old_repo_mount_point = %old_repo_mount_point,
        old_repo = %old_repo.display(),
        "DEBUG: Buck2 get_build_targets paths"
    );

    tracing::debug!("Analyzing changes {mega_changes:?}");

    let subproject = detect_subproject_buck_root(&mount_path, &mega_changes);
    let buck2_root = subproject
        .as_ref()
        .map(|sp| sp.buck_root.clone())
        .unwrap_or_else(|| mount_path.clone());
    let old_buck_root = subproject
        .as_ref()
        .map(|sp| old_repo.join(&sp.strip_prefix))
        .unwrap_or_else(|| old_repo.clone());
    let discovery_changes = subproject
        .as_ref()
        .map(|sp| strip_subproject_changes(&mega_changes, &sp.strip_prefix))
        .unwrap_or_else(|| mega_changes.clone());

    if let Some(sp) = &subproject {
        tracing::info!(
            buck_root = %sp.buck_root.display(),
            strip_prefix = %sp.strip_prefix,
            "Using sub-project .buckconfig for target discovery."
        );
    }

    preheat_shallow(&buck2_root, preheat_shallow_depth())?;

    // DEBUG: Log Buck2 initialization
    tracing::debug!(buck2_root = %buck2_root.display(), "DEBUG: Initializing Buck2 with root");
    let mut buck2 = Buck2::with_root("buck2".to_string(), buck2_root.clone());

    // DEBUG: Log before buck2 cells() call
    tracing::debug!(buck2_root = %buck2_root.display(), "DEBUG: About to call buck2 cells()");
    let cells_result = buck2.cells();

    match &cells_result {
        Ok(_cells_info) => {
            tracing::debug!(buck2_root = %buck2_root.display(), "DEBUG: buck2 cells() succeeded");
        }
        Err(e) => {
            tracing::warn!(buck2_root = %buck2_root.display(), error = %e, "DEBUG: buck2 cells() failed");
        }
    }

    let mut cells =
        CellInfo::parse(&cells_result.map_err(|err| anyhow!("Fail to get cells: {}", err))?)?;

    tracing::debug!("Get config");
    cells.parse_config_data(
        &buck2
            .audit_config()
            .map_err(|err| anyhow!("Fail to get config: {}", err))?,
    )?;

    let full_patterns = cells.get_all_cell_patterns(&buck2_root);
    let all_added = all_changes_are_added(&mega_changes);
    let all_added_subproject = all_added && subproject.is_some();

    let scope = compute_discovery_scope(&cells, &buck2_root, &discovery_changes);
    let project_only_patterns = vec![format!("root//{ALL_ADDED_SUBPROJECT_BUILD_PREFIX}/...")];
    let (discovery_patterns, rdeps_universe) = if all_added_subproject {
        tracing::info!(
            patterns = ?project_only_patterns,
            "All-added subproject import; limiting discovery and build to project/ only."
        );
        (project_only_patterns.clone(), project_only_patterns)
    } else if scope.narrow {
        tracing::info!(
            patterns = ?scope.query_patterns,
            "Using narrowed target discovery scope."
        );
        (scope.query_patterns.clone(), full_patterns.clone())
    } else {
        (full_patterns.clone(), full_patterns.clone())
    };
    let query_patterns = &discovery_patterns;
    let use_rdeps_expansion = all_added_subproject || scope.narrow;
    let graph_rdeps_prefix = if all_added_subproject {
        Some(ALL_ADDED_SUBPROJECT_BUILD_PREFIX)
    } else {
        None
    };

    let base = if all_added {
        tracing::info!(
            change_count = mega_changes.len(),
            "All-added change set; skipping old-repo buck2 targets query."
        );
        Targets::new(Vec::new())
    } else {
        match get_repo_targets(
            "base.jsonl",
            &old_buck_root,
            Some(&cells),
            Some(query_patterns),
        ) {
            Ok(base) => base,
            Err(err) => return Err(err),
        }
    };
    let diff = get_repo_targets(
        "diff.jsonl",
        &buck2_root,
        Some(&cells),
        Some(query_patterns),
    )?;
    let changes = Changes::new(&cells, discovery_changes.clone())?;
    tracing::debug!("Changes {changes:?}");

    tracing::debug!("Base targets number: {}", base.len_targets_upperbound());
    tracing::debug!("Diff targets number: {}", diff.len_targets_upperbound());

    let owner_seed_changes =
        owner_seed_changes_for_discovery(all_added_subproject, all_added, &discovery_changes);

    if all_added_subproject {
        tracing::info!(
            owner_seed_paths = owner_seed_changes.len(),
            "All-added subproject import; seeding targets via owner() on project source paths, then mapping to rust_library/rust_binary."
        );
    } else if all_added {
        tracing::info!(
            owner_seed_paths = owner_seed_changes.len(),
            "All-added change set; skipping graph SelectAll on empty base, seeding targets via owner()."
        );
    }

    let graph_targets = if all_added {
        Vec::new()
    } else {
        collect_impacted_targets(&base, &diff, &changes, diff::EmptyBasePolicy::SelectAll)
    };

    let targets = maybe_expand_narrow_targets(
        &mut buck2,
        &diff,
        &changes,
        graph_targets,
        use_rdeps_expansion,
        &rdeps_universe,
        graph_rdeps_prefix,
    );
    if !targets.is_empty() {
        return Ok(targets);
    }

    let owner_targets = {
        let seeds = fallback_targets_from_owners(&mut buck2, &owner_seed_changes)?;
        let seeds = normalize_owner_targets_to_rust(&diff, seeds);
        maybe_expand_narrow_targets(
            &mut buck2,
            &diff,
            &changes,
            seeds,
            use_rdeps_expansion,
            &rdeps_universe,
            graph_rdeps_prefix,
        )
    };
    if !owner_targets.is_empty() {
        return Ok(owner_targets);
    }

    let (remapped_changes, remapped_count) =
        remap_repo_local_change_paths(&buck2_root, &diff, &owner_seed_changes);
    if remapped_count > 0 {
        let remapped = Changes::new(&cells, remapped_changes.clone())?;
        let remapped_graph_targets = if all_added {
            Vec::new()
        } else {
            collect_impacted_targets(&base, &diff, &remapped, diff::EmptyBasePolicy::SelectAll)
        };
        let remapped_targets = maybe_expand_narrow_targets(
            &mut buck2,
            &diff,
            &remapped,
            remapped_graph_targets,
            use_rdeps_expansion,
            &rdeps_universe,
            graph_rdeps_prefix,
        );
        if !remapped_targets.is_empty() {
            tracing::info!(
                remapped_count,
                recovered_target_count = remapped_targets.len(),
                "Recovered impacted Buck targets after remapping repo-local change paths."
            );
            return Ok(remapped_targets);
        }

        let remapped_owner_seeds =
            owner_seed_changes_for_discovery(all_added_subproject, all_added, &remapped_changes);
        let owner_remapped = {
            let seeds = fallback_targets_from_owners(&mut buck2, &remapped_owner_seeds)?;
            let seeds = normalize_owner_targets_to_rust(&diff, seeds);
            maybe_expand_narrow_targets(
                &mut buck2,
                &diff,
                &remapped,
                seeds,
                use_rdeps_expansion,
                &rdeps_universe,
                graph_rdeps_prefix,
            )
        };
        if !owner_remapped.is_empty() {
            return Ok(owner_remapped);
        }
    }

    Ok(targets)
}

fn validate_project_roots(
    old_project_root: &Path,
    new_project_root: &Path,
) -> anyhow::Result<(String, String)> {
    validate_project_root_exists("old", old_project_root)?;
    validate_project_root_exists("new", new_project_root)?;

    Ok((
        path_to_utf8_string("old", old_project_root)?,
        path_to_utf8_string("new", new_project_root)?,
    ))
}

fn validate_project_root_exists(kind: &str, project_root: &Path) -> anyhow::Result<()> {
    // DEBUG: Log the path being validated
    tracing::debug!(
        kind = kind,
        project_root = %project_root.display(),
        exists = project_root.exists(),
        "DEBUG: Validating project root"
    );

    if project_root.exists() {
        // DEBUG: Check if key cell files exist
        let buckconfig = project_root.join(".buckconfig");
        let buck_file = project_root.join("BUCK");
        tracing::debug!(
            kind = kind,
            project_root = %project_root.display(),
            has_buckconfig = buckconfig.exists(),
            has_buck_file = buck_file.exists(),
            "DEBUG: Project root exists, checking cell markers"
        );
        return Ok(());
    }

    Err(anyhow!(
        "Build repo root ({kind}) does not exist under mounted workspace: {}",
        project_root.display()
    ))
}

fn path_to_utf8_string(kind: &str, path: &Path) -> anyhow::Result<String> {
    path.to_str()
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("Build repo root ({kind}) contains invalid UTF-8: {path:?}"))
}

fn finish_without_build_if_no_targets(
    build_id: &str,
    targets: &[TargetLabel],
    sender: &UnboundedSender<WSMessage>,
) -> anyhow::Result<bool> {
    if !targets.is_empty() {
        return Ok(false);
    }

    sender
        .send(WSMessage::TaskBuildOutput {
            build_id: build_id.to_string(),
            output: "No impacted Buck targets detected for the provided changes.".to_string(),
        })
        .map_err(|e| anyhow!("Failed to send empty-target build output: {e}"))?;
    Ok(true)
}

fn successful_exit_status() -> ExitStatus {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;

        ExitStatus::from_raw(0)
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::ExitStatusExt;

        ExitStatus::from_raw(0)
    }
}

#[derive(Debug)]
pub struct BuildStatusTracker {
    pub cancellation: CancellationToken,
    pub tail_handle: JoinHandle<()>,
    pub process_handle: JoinHandle<()>,
}

/// Start the build status tracker.
/// Returns a `BuildStatusTracker` for lifecycle management.
pub fn start_build_status_tracker(
    project_root: &Path,
    sender: UnboundedSender<WSMessage>,
    cl_id: &str,
    task_id: &str,
) -> BuildStatusTracker {
    let event_jsonl_path = project_root.join(EVENT_LOG_FILE);
    tracing::info!("Track target build status at {:?}", event_jsonl_path);

    let (tx, rx) = mpsc::channel::<String>(4096);
    let cancellation = CancellationToken::new();

    // Spawn tail task: reads the event log file and sends each line into channel
    let tail_handle = {
        let event_log_path = event_jsonl_path.clone();
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            let poll_interval = Duration::from_millis(200);
            if let Err(e) =
                tail_compressed_buck2_events(event_log_path, tx_clone, poll_interval).await
            {
                tracing::error!("Failed to tail buck2 events: {:?}", e);
            }
        })
    };

    // Spawn processing task: handles events, updates state, flushes WebSocket
    let process_handle = {
        let sender_clone = sender.clone();
        let cancellation_clone = cancellation.clone();
        tokio::spawn(run_processing_loop(
            rx,
            sender_clone,
            cancellation_clone,
            cl_id.to_owned(),
            task_id.to_owned(),
        ))
    };

    BuildStatusTracker {
        cancellation,
        tail_handle,
        process_handle,
    }
}

/// Event processing loop.
/// Reads event lines, updates BuildState, and flushes WebSocket in batches or intervals.
async fn run_processing_loop(
    mut rx: mpsc::Receiver<String>,
    sender: UnboundedSender<WSMessage>,
    cancellation: CancellationToken,
    cl_id: String,
    task_id: String,
) {
    let mut build_state = BuildState::default();
    let mut buffer: HashMap<LogicalActionId, TargetBuildStatusUpdate> = HashMap::with_capacity(256);
    let mut flush_interval = interval(Duration::from_millis(FLUSH_INTERVAL_MS));

    loop {
        tokio::select! {
            Some(line) = rx.recv() => {
                match serde_json::from_str::<Event>(&line) {
                    Ok(event) => {
                        if let Some(update) = build_state.handle_event(&event) {
                            buffer.insert(update.action_id.clone(), update);

                             if buffer.len() >= MAX_BATCH_SIZE
                                && !flush_buffer(&sender, &mut buffer, &cl_id, &task_id).await {
                                    break;
                                }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Failed to parse event line: {:?}", e);
                    }
                }
            }

            _ = flush_interval.tick() => {
                if !buffer.is_empty()
                   && !flush_buffer(&sender, &mut buffer, &cl_id, &task_id).await {
                        break;
                    }
            }

            _ = cancellation.cancelled() => {
                tracing::info!("Cancellation received, stopping processing loop");
                break;
            }
        }
    }

    // Flush remaining updates
    if !buffer.is_empty() {
        let _ = flush_buffer(&sender, &mut buffer, &cl_id, &task_id).await;
    }

    tracing::info!("Build status processing loop stopped");
}

/// Flush the buffer of updates to the WebSocket.
/// Returns false if the WebSocket is closed.
async fn flush_buffer(
    sender: &UnboundedSender<WSMessage>,
    buffer: &mut HashMap<LogicalActionId, TargetBuildStatusUpdate>,
    cl_id: &str,
    task_id: &str,
) -> bool {
    if buffer.is_empty() {
        tracing::trace!(task_id, cl_id, "Buffer empty, skipping flush");
        return true;
    }

    let update_count = buffer.len();

    tracing::debug!(
        task_id,
        cl_id,
        update_count,
        "Flushing target build status updates"
    );

    let context = WSBuildContext {
        task_id: task_id.to_string(),
        cl_id: cl_id.to_string(),
    };

    let events: Vec<WSTargetBuildStatusEvent> = buffer
        .drain()
        .map(|(_, update)| WSTargetBuildStatusEvent {
            context: context.clone(),
            target: update.into(),
        })
        .collect();

    let message = WSMessage::TargetBuildStatusBatch { events };

    match sender.send(message) {
        Ok(_) => {
            tracing::trace!(
                task_id,
                cl_id,
                update_count,
                "Successfully flushed target build status updates"
            );
            true
        }
        Err(e) => {
            tracing::warn!(
                task_id,
                cl_id,
                update_count,
                error = %e,
                "Failed to send target build status batch"
            );
            false
        }
    }
}

/// Executes buck build with filesystem mounting and output streaming.
///
/// Process flow:
/// 1. Mount repository filesystem via remote API
/// 2. Execute buck build command with specified target and arguments  
/// 3. Stream build output in real-time via WebSocket
/// 4. Return final build status
///
/// # Arguments
/// * `id` - Build task identifier for logging and tracking
/// * `mount_path` - Monorepo path to mount (Buck2 project root or subdirectory)
/// * `target` - Buck build target specification  
/// * `args` - Additional command-line arguments for buck
/// * `cl` - Change List context identifier
/// * `sender` - WebSocket channel for streaming build output
/// * `changes` - Commit's file change information
///
/// # Returns
/// Process exit status indicating build success or failure
pub async fn build(
    id: String,
    repo: String,
    cl: String,
    sender: UnboundedSender<WSMessage>,
    changes: Vec<Status<ProjectRelativePath>>,
) -> Result<ExitStatus, Box<dyn Error + Send + Sync>> {
    tracing::info!("[Task {}] Building in repo {}", id, repo);

    // Until `buck2 build` starts, the worker only had Busy with phase=None. The UI then
    // looks like the client is not reporting progress during FUSE mount + target discovery.
    if let Err(e) = sender.send(WSMessage::TaskPhaseUpdate {
        build_id: id.clone(),
        phase: TaskPhase::DownloadingSource,
    }) {
        tracing::warn!(
            "[Task {}] Failed to send DownloadingSource phase (server may not show prep progress): {}",
            id,
            e
        );
    }

    let task_id = id.trim();
    // Handle empty cl string as None to mount the base repo without a CL layer.
    let cl_trimmed = cl.trim();
    let cl_arg = (!cl_trimmed.is_empty()).then_some(cl_trimmed);

    // `repo_prefix` is the relative path from monorepo root to the buck2 project.
    // e.g., repo="/project/git-internal/git-internal" → repo_prefix="project/git-internal/git-internal"
    let repo_prefix = repo.strip_prefix('/').unwrap_or(&repo);

    // Task changes follow the hybrid path contract:
    // - files inside `repo` are repo-relative
    // - shared files outside `repo` stay monorepo-relative

    const MAX_TARGETS_ATTEMPTS: usize = 2;
    let retain_mounts = retain_antares_mounts();
    let mut selected_mounts = None;
    let mut targets: Vec<TargetLabel> = Vec::new();
    let mut last_targets_error: Option<anyhow::Error> = None;

    for attempt in 1..=MAX_TARGETS_ATTEMPTS {
        // Mount TWO independent views of the same monorepo:
        //
        //   old_repo  — base revision (no CL), used as the "before" snapshot
        //   new_repo  — base + CL layer,       used as the "after"  snapshot
        //
        // Both mount the same monorepo root (`path = "/"`), but each call to
        // `mount_antares_fs()` creates a **new UUID** on the Antares side, so
        // the mountpoints are different (e.g. `/var/lib/antares/mounts/<uuid>`).
        // Buck2 isolates daemons by project root, so distinct mount paths
        // naturally get separate daemons without needing `--isolation-dir`.
        let id_for_old_repo = format!("{id}-old-{attempt}");
        let (old_repo_mount_point, _mount_id_old_repo) =
            match mount_antares_fs(&id_for_old_repo, &repo, None).await {
                Ok(mount) => mount,
                Err(err) => {
                    cleanup_antares_mount(
                        &id,
                        &id_for_old_repo,
                        None,
                        "cleanup after failed old-repo mount",
                    )
                    .await;
                    return Err(err);
                }
            };

        let id_for_repo = format!("{id}-{attempt}");
        let (repo_mount_point, _mount_id) =
            match mount_antares_fs(&id_for_repo, &repo, cl_arg).await {
                Ok(mount) => mount,
                Err(err) => {
                    cleanup_antares_mount(
                        &id,
                        &id_for_old_repo,
                        Some(&old_repo_mount_point),
                        "cleanup old-repo mount after failed new-repo mount",
                    )
                    .await;
                    cleanup_antares_mount(
                        &id,
                        &id_for_repo,
                        None,
                        "cleanup after failed new-repo mount",
                    )
                    .await;
                    return Err(err);
                }
            };

        let attempt_mounts = AntaresMountPair {
            old_mount_id: id_for_old_repo,
            old_mount_point: old_repo_mount_point.clone(),
            new_mount_id: id_for_repo,
            new_mount_point: repo_mount_point.clone(),
            old_unmounted: false,
        };

        tracing::info!(
            "[Task {}] Filesystem mounted successfully (attempt {}/{}).",
            id,
            attempt,
            MAX_TARGETS_ATTEMPTS
        );

        // Resolve the sub-project paths within each mount for buck2.
        let old_project_root = PathBuf::from(&old_repo_mount_point).join(repo_prefix);
        let new_project_root = PathBuf::from(&repo_mount_point).join(repo_prefix);

        let project_roots = validate_project_roots(&old_project_root, &new_project_root);
        let (old_project_root_str, new_project_root_str) = match project_roots {
            Ok(roots) => roots,
            Err(e) => {
                tracing::warn!(
                    "[Task {}] Invalid project roots (attempt {}/{}): {}. old_root={}, new_root={}",
                    id,
                    attempt,
                    MAX_TARGETS_ATTEMPTS,
                    e,
                    old_project_root.display(),
                    new_project_root.display(),
                );
                last_targets_error = Some(e);
                if !retain_mounts {
                    cleanup_antares_mount_pair(
                        &id,
                        &attempt_mounts,
                        "cleanup after invalid project roots",
                    )
                    .await;
                }
                if attempt == MAX_TARGETS_ATTEMPTS {
                    break;
                }
                continue;
            }
        };

        match get_build_targets(
            &old_project_root_str,
            &new_project_root_str,
            changes.clone(),
        )
        .await
        {
            Ok(found_targets) => {
                selected_mounts = Some(attempt_mounts);
                tracing::info!(
                    "[Task {}] Target discovery succeeded: {} targets",
                    id,
                    found_targets.len()
                );
                targets = found_targets;
                break;
            }
            Err(e) => {
                if retain_mounts {
                    tracing::warn!(
                        "[Task {}] Failed to get build targets (attempt {}/{}): {}. Mounts retained for debugging (old={}, new={}).",
                        id,
                        attempt,
                        MAX_TARGETS_ATTEMPTS,
                        e,
                        attempt_mounts.old_mount_point,
                        attempt_mounts.new_mount_point,
                    );
                } else {
                    tracing::warn!(
                        "[Task {}] Failed to get build targets (attempt {}/{}): {}. Cleaning stale Antares mounts before retry.",
                        id,
                        attempt,
                        MAX_TARGETS_ATTEMPTS,
                        e,
                    );
                    cleanup_antares_mount_pair(
                        &id,
                        &attempt_mounts,
                        "cleanup after target discovery failure",
                    )
                    .await;
                }
                last_targets_error = Some(e);
                if attempt == MAX_TARGETS_ATTEMPTS {
                    break;
                }
            }
        }
    }

    let mut mounts = match selected_mounts {
        Some(value) => value,
        None => {
            let err = last_targets_error
                .map(|e| anyhow!("Error getting build targets: {e}"))
                .unwrap_or_else(|| anyhow!("Error getting build targets: mount point missing"));
            let error_msg = err.to_string();
            if sender
                .send(WSMessage::TaskBuildOutput {
                    build_id: id.clone(),
                    output: error_msg.clone(),
                })
                .is_err()
            {
                tracing::error!(
                    "[Task {}] Failed to send BuildOutput for target discovery error",
                    id
                );
            }
            return Err(err.into());
        }
    };
    let mount_point = mounts.new_mount_point.clone();

    if !retain_mounts {
        unmount_discovery_old_mount(&id, &mut mounts).await;
    }

    if finish_without_build_if_no_targets(&id, &targets, &sender)? {
        tracing::info!(
            "[Task {}] No impacted Buck targets detected; skipping buck2 build.",
            id
        );
        if retain_mounts {
            tracing::info!(
                "[Task {}] Skipped build and retained Antares mounts for debugging: new_repo mountpoint={}; old_repo mountpoint={}",
                id,
                mounts.new_mount_point,
                mounts.old_mount_point,
            );
        } else {
            cleanup_antares_mount_pair(&id, &mounts, "cleanup after no-op target set").await;
        }
        return Ok(successful_exit_status());
    }

    let build_result = async {
        // Run buck2 build from the sub-project directory when it has its own `.buckconfig`.
        let mut project_root = PathBuf::from(&mount_point).join(repo_prefix);
        if let Some(sp) = detect_subproject_buck_root(&project_root, &changes) {
            tracing::info!(
                buck_root = %sp.buck_root.display(),
                strip_prefix = %sp.strip_prefix,
                "Using sub-project .buckconfig for buck2 build."
            );
            project_root = sp.buck_root;
        }
        tracing::info!(
            "[Task {}] Starting buck2 build. project_root={}, targets={}",
            id,
            project_root.display(),
            targets.len()
        );

        // Kill daemon + unique isolation-dir limit cross-build local cache sharing.
        // Remote cache is off by default; set ORION_BUCK_REMOTE_CACHE=1 to enable it.
        // Note: Buck2 requires isolation-dir to be a simple directory name without path separators
        let isolation_dir = format!("buck-isolation-{}", id);
        let remote_cache = buck_remote_cache_enabled();
        let mut kill_cmd = Command::new("buck2");
        kill_cmd
            .arg("kill")
            .arg("--isolation-dir")
            .arg(&isolation_dir)
            .current_dir(&project_root)
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        match kill_cmd.status().await {
            Ok(status) if !status.success() => {
                tracing::debug!("[Task {}] Buck2 daemon was not running (expected)", id);
            }
            Err(e) => {
                tracing::warn!("[Task {}] Failed to kill buck2 daemon: {}", id, e);
            }
            _ => {}
        }

        // Wait for daemon to fully stop before starting a new build
        tokio::time::sleep(Duration::from_millis(500)).await;

        let mut cmd = Command::new("buck2");
        // --event-log and --build-report are used to collect build execution status
        // at target level (e.g. pending / running / succeeded / failed).
        // FUSE-backed repos may trigger lazy loading during daemon init, which
        // can be slow on cold caches — allow up to 1200s for the daemon to start.
        cmd.env("BUCKD_STARTUP_TIMEOUT", "30")
            .env("BUCKD_STARTUP_INIT_TIMEOUT", "1200")
            .arg("build");
        for flag in platform_config_flags() {
            cmd.arg(flag);
        }
        cmd.args(["--event-log", EVENT_LOG_FILE])
            .args(&targets)
            // Avoid failing the whole build when a target is explicitly incompatible
            // with the selected platform (e.g., macOS-only crates on Linux builders).
            .arg("--skip-incompatible-targets")
            .arg("--verbose=2");
        if remote_cache {
            tracing::info!(
                "[Task {}] ORION_BUCK_REMOTE_CACHE enabled; buck2 may read remote action cache.",
                id
            );
        } else {
            cmd.arg("--no-remote-cache");
        }
        cmd.arg("--isolation-dir")
            .arg(&isolation_dir)
            .current_dir(&project_root)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        tracing::debug!("[Task {}] Executing command: {:?}", id, cmd);

        let mut child = cmd.spawn().map_err(|e| {
            tracing::error!(
                "[Task {}] Failed to spawn buck2 (cwd={}): {}",
                id,
                project_root.display(),
                e
            );
            e
        })?;

        if let Err(e) = sender.send(WSMessage::TaskPhaseUpdate {
            build_id: id.clone(),
            phase: TaskPhase::RunningBuild,
        }) {
            tracing::error!("Failed to send RunningBuild phase update: {}", e);
        }

        tracing::info!(
            "[Task {}] Starting buck2 event-log tracker: file={}",
            id,
            project_root.join(EVENT_LOG_FILE).display()
        );
        let target_build_track =
            start_build_status_tracker(&project_root, sender.clone(), cl_trimmed, task_id);

        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let mut stdout_reader = tokio::io::BufReader::new(stdout).lines();
        let mut stderr_reader = tokio::io::BufReader::new(stderr).lines();

        let mut exit_status: Option<ExitStatus> = None;
        loop {
            tokio::select! {
                result = stdout_reader.next_line() => {
                    match result {
                        Ok(Some(line)) => {
                            // Log at info so run.sh/orion.log and scheduler SSE can tail build output.
                            tracing::info!("[Task {}] buck2: {}", id, line);
                            if sender.send(WSMessage::TaskBuildOutput { build_id: id.clone(), output: line }).is_err() {
                                child.kill().await?;
                                return Err("WebSocket connection lost during build.".into());
                            }
                        },
                        Ok(None) => break,
                        Err(e) => {
                            tracing::error!("[Task {}] Error reading stdout: {}", id, e);
                            break;
                        }
                    }
                },
                result = stderr_reader.next_line() => {
                    match result {
                        Ok(Some(line)) => {
                            // Log buck2 stderr for debugging and error tracking
                            tracing::warn!("[Task {}] buck2 stderr: {}", id, line);
                            if sender.send(WSMessage::TaskBuildOutput { build_id: id.clone(), output: line }).is_err() {
                                child.kill().await?;
                                return Err("WebSocket connection lost during build.".into());
                            }
                        },
                        Ok(None) => break,
                        Err(e) => {
                            tracing::error!("[Task {}] Error reading stderr: {}", id, e);
                            break;
                        },
                    }
                },
                status = child.wait() => {
                    let status = status?;
                    tracing::info!("[Task {}] Buck2 process finished with status: {}", id, status);
                    exit_status = Some(status);
                    break;
                }
            }
        }

        let status = match exit_status {
            Some(s) => s,
            None => child.wait().await?,
        };

        // Log build result for debugging
        if status.success() {
            tracing::info!("[Task {}] Buck2 build completed successfully", id);
        } else {
            tracing::error!(
                "[Task {}] Buck2 build failed with exit code: {}",
                id,
                status.code().map_or("unknown".to_string(), |c| c.to_string())
            );
        }

        // Stop the build-status tracker cleanly.
        // Signal the processing loop to exit via cancellation token; it will
        // flush its remaining buffer in the cleanup path after breaking out of
        // the select! loop.
        target_build_track.cancellation.cancel();
        // Abort the tail task — no more events need to be ingested.
        target_build_track.tail_handle.abort();
        let _ = target_build_track.tail_handle.await;
        // Give the processing loop a short grace period to flush its final
        // buffered updates after observing the cancellation signal.
        if tokio::time::timeout(
            Duration::from_secs(2),
            target_build_track.process_handle,
        )
        .await
        .is_err()
        {
            tracing::warn!("Build status processing loop did not finish within 2s grace period");
        }
        tracing::info!("Target build status track finished");
        Ok(status)
    }
    .await;

    // Cleanup isolation directory to prevent disk space leak
    // Buck2 creates the isolation directory in the project root
    let isolation_dir = format!("buck-isolation-{}", id);
    let isolation_path = PathBuf::from(&mount_point)
        .join(repo_prefix)
        .join(&isolation_dir);
    if isolation_path.exists()
        && let Err(e) = tokio::fs::remove_dir_all(&isolation_path).await
    {
        tracing::warn!("[Task {}] Failed to cleanup isolation-dir: {}", id, e);
    }

    if retain_mounts {
        tracing::info!(
            "[Task {}] Build completed — mount directories retained for debugging: \
             new_repo mountpoint={}; \
             old_repo mountpoint={}",
            id,
            mounts.new_mount_point,
            mounts.old_mount_point,
        );
    } else {
        cleanup_antares_mount_pair(&id, &mounts, "cleanup after build completion").await;
    }

    build_result
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        time::Duration,
    };

    use api_model::buck2::{status::Status, types::ProjectRelativePath};
    use serial_test::serial;
    use td_util_buck::{
        targets::{BuckTarget, Targets, TargetsEntry},
        types::{RuleType, TargetLabel},
    };
    use tempfile::TempDir;
    use tokio::sync::mpsc;

    use super::{
        all_changes_are_added, antares_unmount_grace_duration, buck_remote_cache_enabled,
        filter_owner_seed_changes, finish_without_build_if_no_targets, get_build_targets,
        get_repo_targets, is_toolchain_or_platform_rule, is_toolchain_or_platform_target,
        label_is_toolchain_or_platform, normalize_owner_targets_to_rust,
        owner_seed_changes_for_discovery, remap_repo_local_change_paths, retain_antares_mounts,
        validate_project_root_exists,
    };

    struct JsonlCleanupGuard {
        paths: Vec<PathBuf>,
    }

    impl JsonlCleanupGuard {
        fn new(paths: impl IntoIterator<Item = PathBuf>) -> Self {
            Self {
                paths: paths.into_iter().collect(),
            }
        }
    }

    impl Drop for JsonlCleanupGuard {
        fn drop(&mut self) {
            for path in &self.paths {
                let _ = fs::remove_file(path);
            }
        }
    }

    fn workspace_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("workspace root")
            .to_path_buf()
    }

    fn subproject_root(relative: &str) -> PathBuf {
        workspace_root().join(relative)
    }

    fn path_exists(path: &Path) -> bool {
        path.exists()
    }

    fn copy_dir_all(src: &Path, dst: &Path) {
        fs::create_dir_all(dst).expect("create fixture directory");
        for entry in fs::read_dir(src).expect("read fixture directory") {
            let entry = entry.expect("read fixture entry");
            let ty = entry.file_type().expect("read fixture type");
            let dst_path = dst.join(entry.file_name());
            if ty.is_dir() {
                copy_dir_all(&entry.path(), &dst_path);
            } else {
                fs::copy(entry.path(), dst_path).expect("copy fixture file");
            }
        }
    }

    fn isolated_fixture(relative: &str) -> (TempDir, PathBuf, PathBuf) {
        let fixture_root = subproject_root(relative);
        let tempdir = TempDir::new().expect("create tempdir");
        let old_root = tempdir.path().join("old");
        let new_root = tempdir.path().join("new");
        copy_dir_all(&fixture_root, &old_root);
        copy_dir_all(&fixture_root, &new_root);
        (tempdir, old_root, new_root)
    }

    fn isolated_buck_scope_fixture() -> (TempDir, PathBuf, PathBuf) {
        isolated_fixture("orion/tests/fixtures/change_detector_buck_scope")
    }

    fn isolated_ambiguous_main_fixture() -> (TempDir, PathBuf, PathBuf) {
        isolated_fixture("orion/tests/fixtures/change_detector_ambiguous_main")
    }

    fn set_mount_retention_env(value: Option<&str>) {
        // SAFETY: these tests are marked serial and only mutate process env inside the test.
        unsafe {
            match value {
                Some(value) => std::env::set_var("ORION_RETAIN_ANTARES_MOUNTS", value),
                None => std::env::remove_var("ORION_RETAIN_ANTARES_MOUNTS"),
            }
        }
    }

    fn set_antares_unmount_grace_env(value: Option<&str>) {
        // SAFETY: these tests are marked serial and only mutate process env inside the test.
        unsafe {
            match value {
                Some(value) => std::env::set_var("ORION_ANTARES_UNMOUNT_GRACE_MS", value),
                None => std::env::remove_var("ORION_ANTARES_UNMOUNT_GRACE_MS"),
            }
        }
    }

    #[test]
    fn test_toolchain_and_platform_rules_are_detected() {
        for rule in [
            "prelude//toolchains/jdk.bzl:system_jdk_toolchain",
            "prelude//rules.bzl:toolchain",
            "prelude//platforms.bzl:platform",
            "prelude//config.bzl:config_setting",
            "prelude//config.bzl:constraint_value",
        ] {
            assert!(
                is_toolchain_or_platform_rule(&RuleType::new(rule)),
                "expected {rule} to be treated as toolchain/platform plumbing"
            );
        }

        for rule in [
            "prelude//rules.bzl:rust_library",
            "prelude//rules.bzl:rust_binary",
            "prelude//rules.bzl:genrule",
        ] {
            assert!(
                !is_toolchain_or_platform_rule(&RuleType::new(rule)),
                "expected {rule} to be treated as a business rule"
            );
        }
    }

    #[test]
    fn test_toolchain_helper_targets_are_excluded() {
        let jdk = BuckTarget::testing(
            "jdk_system_image",
            "root//project/buck2_test/toolchains",
            "prelude//toolchains/jdk.bzl:create_jdk_system_image",
        );
        assert!(is_toolchain_or_platform_target(&jdk));

        let android = BuckTarget::testing(
            "__android_sdk_tools__",
            "root//rk8s/foo",
            "prelude//rules.bzl:genrule",
        );
        assert!(is_toolchain_or_platform_target(&android));

        let rk8s_toolchain = BuckTarget::testing(
            "rust-platform",
            "root//rk8s/toolchains",
            "prelude//rules.bzl:platform",
        );
        assert!(is_toolchain_or_platform_target(&rk8s_toolchain));

        let rust_lib = BuckTarget::testing(
            "jni",
            "root//rk8s/third-party/rust/crates/jni/0.21.1",
            "prelude//rules.bzl:rust_library",
        );
        assert!(
            !is_toolchain_or_platform_target(&rust_lib),
            "a real rust crate must not be filtered even if it binds to the JVM"
        );
    }

    #[test]
    fn test_label_toolchain_detection_for_fallback() {
        assert!(label_is_toolchain_or_platform(&TargetLabel::new(
            "root//project/buck2_test/toolchains:jdk_system_image"
        )));
        assert!(label_is_toolchain_or_platform(&TargetLabel::new(
            "toolchains//:cxx"
        )));
        assert!(!label_is_toolchain_or_platform(&TargetLabel::new(
            "root//rk8s/src:lib"
        )));
    }

    #[test]
    #[serial]
    fn test_retain_antares_mounts_defaults_to_false() {
        set_mount_retention_env(None);
        assert!(!retain_antares_mounts());
    }

    #[test]
    #[serial]
    fn test_retain_antares_mounts_accepts_truthy_values() {
        set_mount_retention_env(Some("true"));
        assert!(retain_antares_mounts());
        set_mount_retention_env(None);
    }

    #[test]
    #[serial]
    fn test_retain_antares_mounts_treats_falsey_values_as_disabled() {
        for value in ["", "0", "false", "no", "off"] {
            set_mount_retention_env(Some(value));
            assert!(
                !retain_antares_mounts(),
                "expected {value:?} to disable Antares mount retention"
            );
        }
        set_mount_retention_env(None);
    }

    #[test]
    #[serial]
    fn test_antares_unmount_grace_duration_defaults_to_150ms() {
        set_antares_unmount_grace_env(None);
        assert_eq!(antares_unmount_grace_duration(), Duration::from_millis(150));
    }

    #[test]
    #[serial]
    fn test_antares_unmount_grace_duration_accepts_explicit_value() {
        set_antares_unmount_grace_env(Some("275"));
        assert_eq!(antares_unmount_grace_duration(), Duration::from_millis(275));
        set_antares_unmount_grace_env(None);
    }

    #[test]
    #[serial]
    fn test_antares_unmount_grace_duration_clamps_and_falls_back() {
        set_antares_unmount_grace_env(Some("50000"));
        assert_eq!(
            antares_unmount_grace_duration(),
            Duration::from_millis(3000)
        );

        set_antares_unmount_grace_env(Some("not-a-number"));
        assert_eq!(antares_unmount_grace_duration(), Duration::from_millis(150));

        set_antares_unmount_grace_env(None);
    }

    #[tokio::test]
    #[serial]
    async fn test_get_build_targets_detects_root_relative_subproject_source_change() {
        let subproject_relative = "jupiter/callisto";
        let subproject_root = subproject_root(subproject_relative);
        assert!(
            path_exists(&subproject_root.join("BUCK")),
            "expected test fixture project to exist at {:?}",
            subproject_root
        );

        let _cleanup = JsonlCleanupGuard::new([
            subproject_root.join("base.jsonl"),
            subproject_root.join("diff.jsonl"),
        ]);

        let targets = get_build_targets(
            subproject_root.to_str().expect("subproject root path"),
            subproject_root.to_str().expect("subproject root path"),
            vec![Status::Modified(ProjectRelativePath::new(
                "jupiter/callisto/src/access_token.rs",
            ))],
        )
        .await
        .expect("target discovery should complete");

        assert!(
            targets.contains(&TargetLabel::new("root//jupiter/callisto:callisto")),
            "expected source change to rebuild root//jupiter/callisto:callisto, got {targets:?}"
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_get_build_targets_keeps_repo_relative_shared_dependency_changes() {
        let subproject_relative = "orion/tests/fixtures/change_detector_mixed/app";
        let subproject_root = subproject_root(subproject_relative);
        assert!(
            path_exists(&subproject_root.join("BUCK")),
            "expected fixture project to exist at {:?}",
            subproject_root
        );

        let _cleanup = JsonlCleanupGuard::new([
            subproject_root.join("base.jsonl"),
            subproject_root.join("diff.jsonl"),
        ]);

        let targets = get_build_targets(
            subproject_root.to_str().expect("subproject root path"),
            subproject_root.to_str().expect("subproject root path"),
            vec![Status::Modified(ProjectRelativePath::new(
                "orion/tests/fixtures/change_detector_mixed/shared/src/lib.rs",
            ))],
        )
        .await
        .expect("target discovery should complete");

        assert!(
            targets.contains(&TargetLabel::new(
                "root//orion/tests/fixtures/change_detector_mixed/app:app"
            )),
            "expected shared dependency change to rebuild root//orion/tests/fixtures/change_detector_mixed/app:app, got {targets:?}"
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_get_build_targets_handles_mixed_repo_root_relative_changes() {
        let subproject_relative = "orion/tests/fixtures/change_detector_mixed/app";
        let subproject_root = subproject_root(subproject_relative);
        assert!(
            path_exists(&subproject_root.join("README.md")),
            "expected fixture file to exist at {:?}",
            subproject_root.join("README.md")
        );

        let _cleanup = JsonlCleanupGuard::new([
            subproject_root.join("base.jsonl"),
            subproject_root.join("diff.jsonl"),
        ]);

        let targets = get_build_targets(
            subproject_root.to_str().expect("subproject root path"),
            subproject_root.to_str().expect("subproject root path"),
            vec![
                Status::Modified(ProjectRelativePath::new(
                    "orion/tests/fixtures/change_detector_mixed/app/README.md",
                )),
                Status::Modified(ProjectRelativePath::new(
                    "orion/tests/fixtures/change_detector_mixed/shared/src/lib.rs",
                )),
            ],
        )
        .await
        .expect("target discovery should complete");

        assert!(
            targets.contains(&TargetLabel::new(
                "root//orion/tests/fixtures/change_detector_mixed/app:app"
            )),
            "expected mixed change list to rebuild root//orion/tests/fixtures/change_detector_mixed/app:app, got {targets:?}"
        );
    }

    #[test]
    fn test_all_changes_are_added_requires_non_empty_all_added_set() {
        assert!(all_changes_are_added(&[
            Status::Added(ProjectRelativePath::new(".buckconfig")),
            Status::Added(ProjectRelativePath::new("src/main.rs")),
        ]));
        assert!(!all_changes_are_added(&[]));
        assert!(!all_changes_are_added(&[
            Status::Added(ProjectRelativePath::new("src/main.rs")),
            Status::Modified(ProjectRelativePath::new("src/lib.rs")),
        ]));
    }

    #[tokio::test]
    #[serial]
    async fn test_get_build_targets_detects_modified_tracked_file_in_standalone_repo() {
        let (_tempdir, old_root, new_root) = isolated_buck_scope_fixture();
        fs::write(
            new_root.join("src/main.rs"),
            "fn main() { println!(\"modified fixture main\"); }\n",
        )
        .expect("rewrite main.rs");

        let targets = get_build_targets(
            old_root.to_str().expect("old fixture path"),
            new_root.to_str().expect("new fixture path"),
            vec![Status::Modified(ProjectRelativePath::new("src/main.rs"))],
        )
        .await
        .expect("target discovery should complete");

        assert!(
            targets.contains(&TargetLabel::new("root//:explicit_main")),
            "expected tracked main.rs change to rebuild root//:explicit_main, got {targets:?}"
        );
        assert!(
            targets.contains(&TargetLabel::new("root//:globbed_lib")),
            "expected tracked main.rs change to rebuild root//:globbed_lib, got {targets:?}"
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_get_build_targets_remaps_truncated_repo_local_path() {
        let (_tempdir, old_root, new_root) = isolated_buck_scope_fixture();
        fs::write(
            new_root.join("src/main.rs"),
            "fn main() { println!(\"remapped fixture main\"); }\n",
        )
        .expect("rewrite main.rs");

        let targets = get_build_targets(
            old_root.to_str().expect("old fixture path"),
            new_root.to_str().expect("new fixture path"),
            vec![Status::Modified(ProjectRelativePath::new("main.rs"))],
        )
        .await
        .expect("target discovery should complete");

        assert!(
            targets.contains(&TargetLabel::new("root//:explicit_main")),
            "expected remapped main.rs change to rebuild root//:explicit_main, got {targets:?}"
        );
        assert!(
            targets.contains(&TargetLabel::new("root//:globbed_lib")),
            "expected remapped main.rs change to rebuild root//:globbed_lib, got {targets:?}"
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_get_build_targets_detects_added_file_inside_buck_glob_scope() {
        let (_tempdir, old_root, new_root) = isolated_buck_scope_fixture();
        let new_file = new_root.join("src/generated/new_module.rs");
        fs::create_dir_all(
            new_file
                .parent()
                .expect("new file should have a parent directory"),
        )
        .expect("create generated dir");
        fs::write(new_file, "pub fn generated() -> &'static str { \"ok\" }\n")
            .expect("write generated rust file");

        let targets = get_build_targets(
            old_root.to_str().expect("old fixture path"),
            new_root.to_str().expect("new fixture path"),
            vec![Status::Added(ProjectRelativePath::new(
                "src/generated/new_module.rs",
            ))],
        )
        .await
        .expect("target discovery should complete");

        assert!(
            targets.contains(&TargetLabel::new("root//:globbed_lib")),
            "expected added globbed source to rebuild root//:globbed_lib, got {targets:?}"
        );
        assert!(
            !targets.contains(&TargetLabel::new("root//:explicit_main")),
            "explicit target should not rebuild for unrelated added file, got {targets:?}"
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_get_build_targets_detects_buckroot_change_as_package_level_impact() {
        let (_tempdir, old_root, new_root) = isolated_buck_scope_fixture();
        fs::write(new_root.join(".buckroot"), "# touched by test\n").expect("rewrite .buckroot");

        let targets = get_build_targets(
            old_root.to_str().expect("old fixture path"),
            new_root.to_str().expect("new fixture path"),
            vec![Status::Modified(ProjectRelativePath::new(".buckroot"))],
        )
        .await
        .expect("target discovery should complete");

        assert!(
            targets.contains(&TargetLabel::new("root//:explicit_main")),
            "expected .buckroot change to rebuild root//:explicit_main, got {targets:?}"
        );
        assert!(
            targets.contains(&TargetLabel::new("root//:globbed_lib")),
            "expected .buckroot change to rebuild root//:globbed_lib, got {targets:?}"
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_get_build_targets_detects_cargo_toml_change_as_package_level_impact() {
        let (_tempdir, old_root, new_root) = isolated_buck_scope_fixture();
        fs::write(
            new_root.join("Cargo.toml"),
            r#"[package]
name = "change_detector_buck_scope"
version = "0.1.0"
edition = "2024"

[dependencies]
# touched by test
"#,
        )
        .expect("rewrite Cargo.toml");

        let targets = get_build_targets(
            old_root.to_str().expect("old fixture path"),
            new_root.to_str().expect("new fixture path"),
            vec![Status::Modified(ProjectRelativePath::new("Cargo.toml"))],
        )
        .await
        .expect("target discovery should complete");

        assert!(
            targets.contains(&TargetLabel::new("root//:explicit_main")),
            "expected Cargo.toml change to rebuild root//:explicit_main, got {targets:?}"
        );
        assert!(
            targets.contains(&TargetLabel::new("root//:globbed_lib")),
            "expected Cargo.toml change to rebuild root//:globbed_lib, got {targets:?}"
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_get_build_targets_recovers_targets_when_changed_buck_file_has_parse_error() {
        let (_tempdir, old_root, new_root) = isolated_buck_scope_fixture();
        fs::write(
            new_root.join("BUCK"),
            r#"rust_binary(
    name = "broken",
"#,
        )
        .expect("rewrite BUCK with parse error");

        let targets = get_build_targets(
            old_root.to_str().expect("old fixture path"),
            new_root.to_str().expect("new fixture path"),
            vec![Status::Modified(ProjectRelativePath::new("BUCK"))],
        )
        .await
        .expect("target discovery should recover fallback targets from base graph");

        assert!(
            targets.contains(&TargetLabel::new("root//:explicit_main")),
            "expected fallback to keep root//:explicit_main when BUCK parse fails, got {targets:?}"
        );
        assert!(
            targets.contains(&TargetLabel::new("root//:globbed_lib")),
            "expected fallback to keep root//:globbed_lib when BUCK parse fails, got {targets:?}"
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_get_build_targets_ignores_added_file_outside_buck_scope() {
        let (_tempdir, old_root, new_root) = isolated_buck_scope_fixture();
        let new_file = new_root.join("notes/added.txt");
        fs::create_dir_all(
            new_file
                .parent()
                .expect("new file should have a parent directory"),
        )
        .expect("create notes dir");
        fs::write(new_file, "not referenced by any Buck target\n").expect("write non-buck file");

        let targets = get_build_targets(
            old_root.to_str().expect("old fixture path"),
            new_root.to_str().expect("new fixture path"),
            vec![Status::Added(ProjectRelativePath::new("notes/added.txt"))],
        )
        .await
        .expect("target discovery should complete");

        assert!(
            targets.is_empty(),
            "expected no targets for out-of-scope added file, got {targets:?}"
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_get_build_targets_filters_unsafe_paths_and_keeps_valid_change() {
        let (_tempdir, old_root, new_root) = isolated_buck_scope_fixture();
        fs::write(
            new_root.join("src/main.rs"),
            "fn main() { println!(\"unsafe-filter fixture main\"); }\n",
        )
        .expect("rewrite main.rs");

        let targets = get_build_targets(
            old_root.to_str().expect("old fixture path"),
            new_root.to_str().expect("new fixture path"),
            vec![
                Status::Modified(ProjectRelativePath::new("../secret.rs")),
                Status::Modified(ProjectRelativePath::new("project//buck2_test/src/main.rs")),
                Status::Modified(ProjectRelativePath::new("src/main.rs")),
            ],
        )
        .await
        .expect("target discovery should complete");

        assert!(
            targets.contains(&TargetLabel::new("root//:explicit_main")),
            "expected valid repo-local change to rebuild root//:explicit_main, got {targets:?}"
        );
        assert!(
            targets.contains(&TargetLabel::new("root//:globbed_lib")),
            "expected valid repo-local change to rebuild root//:globbed_lib, got {targets:?}"
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_remap_repo_local_change_paths_preserves_removed_and_added_statuses() {
        let (_tempdir, _old_root, new_root) = isolated_buck_scope_fixture();
        let jsonl_cleanup =
            JsonlCleanupGuard::new([new_root.join("diff.jsonl"), new_root.join("base.jsonl")]);
        let diff =
            get_repo_targets("diff.jsonl", &new_root, None, None).expect("load diff targets");

        let (remapped, remapped_count) = remap_repo_local_change_paths(
            &new_root,
            &diff,
            &[
                Status::Removed(ProjectRelativePath::new("main.rs")),
                Status::Added(ProjectRelativePath::new("lib.rs")),
            ],
        );

        drop(jsonl_cleanup);
        assert_eq!(remapped_count, 2);
        assert_eq!(
            remapped,
            vec![
                Status::Removed(ProjectRelativePath::new("src/main.rs")),
                Status::Added(ProjectRelativePath::new("src/lib.rs")),
            ]
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_get_build_targets_does_not_remap_ambiguous_truncated_path() {
        let (_tempdir, old_root, new_root) = isolated_ambiguous_main_fixture();
        fs::write(
            new_root.join("src/main.rs"),
            "fn main() { println!(\"ambiguous src main\"); }\n",
        )
        .expect("rewrite src/main.rs");
        fs::write(
            new_root.join("examples/main.rs"),
            "pub fn run() { println!(\"ambiguous examples main\"); }\n",
        )
        .expect("rewrite examples/main.rs");

        let targets = get_build_targets(
            old_root.to_str().expect("old fixture path"),
            new_root.to_str().expect("new fixture path"),
            vec![Status::Modified(ProjectRelativePath::new("main.rs"))],
        )
        .await
        .expect("target discovery should complete");

        assert!(
            targets.is_empty(),
            "expected ambiguous short path to avoid remap and keep target list empty, got {targets:?}"
        );
    }

    #[test]
    fn test_finish_without_build_if_no_targets_emits_clear_message() {
        let (sender, mut receiver) = mpsc::unbounded_channel();

        let skipped = finish_without_build_if_no_targets("build-1", &[], &sender)
            .expect("empty targets should short-circuit");

        assert!(skipped);
        match receiver.try_recv().expect("expected a websocket message") {
            api_model::buck2::ws::WSMessage::TaskBuildOutput { build_id, output } => {
                assert_eq!(build_id, "build-1");
                assert_eq!(
                    output,
                    "No impacted Buck targets detected for the provided changes."
                );
            }
            other => panic!("unexpected websocket message: {other:?}"),
        }
    }

    #[test]
    fn test_validate_project_root_exists_returns_error_for_missing_path() {
        let tempdir = TempDir::new().expect("create tempdir");
        let missing = tempdir.path().join("missing/subproject");

        let err = validate_project_root_exists("new", &missing).unwrap_err();
        assert!(
            err.to_string()
                .contains("Build repo root (new) does not exist"),
            "unexpected error: {err}"
        );
    }

    fn set_buck_remote_cache_env(value: Option<&str>) {
        // SAFETY: these tests are marked serial and only mutate process env inside the test.
        unsafe {
            match value {
                Some(value) => std::env::set_var("ORION_BUCK_REMOTE_CACHE", value),
                None => std::env::remove_var("ORION_BUCK_REMOTE_CACHE"),
            }
        }
    }

    #[test]
    #[serial]
    fn test_buck_remote_cache_disabled_by_default() {
        set_buck_remote_cache_env(None);
        assert!(!buck_remote_cache_enabled());
    }

    #[test]
    #[serial]
    fn test_buck_remote_cache_enabled_with_one() {
        set_buck_remote_cache_env(Some("1"));
        assert!(buck_remote_cache_enabled());
        set_buck_remote_cache_env(None);
    }

    #[test]
    fn test_filter_owner_seed_changes_skips_buck_and_vendor() {
        let changes = vec![
            Status::Added(ProjectRelativePath::new("project/common/src/lib.rs")),
            Status::Added(ProjectRelativePath::new("project/common/BUCK")),
            Status::Added(ProjectRelativePath::new("project/common/vendor/foo.rs")),
        ];
        let filtered = filter_owner_seed_changes(&changes);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].get().as_str(), "project/common/src/lib.rs");
    }

    #[test]
    fn test_owner_seed_changes_for_all_added_non_subproject_skips_select_all_path() {
        let changes = vec![
            Status::Added(ProjectRelativePath::new("orion/src/lib.rs")),
            Status::Added(ProjectRelativePath::new("orion/BUCK")),
        ];
        let seeds = owner_seed_changes_for_discovery(false, true, &changes);
        assert_eq!(seeds.len(), 1);
        assert_eq!(seeds[0].get().as_str(), "orion/src/lib.rs");
    }

    #[test]
    fn test_owner_seed_changes_for_modified_cl_uses_full_change_list() {
        let changes = vec![
            Status::Modified(ProjectRelativePath::new("orion/src/lib.rs")),
            Status::Added(ProjectRelativePath::new("orion/BUCK")),
        ];
        let seeds = owner_seed_changes_for_discovery(false, false, &changes);
        assert_eq!(seeds.len(), 2);
    }

    #[test]
    fn test_normalize_owner_targets_maps_vendor_to_rust_library() {
        let diff = Targets::new(vec![
            TargetsEntry::Target(BuckTarget::testing(
                "vendor",
                "root//project/common",
                "filegroup",
            )),
            TargetsEntry::Target(BuckTarget::testing(
                "common",
                "root//project/common",
                "rust_library",
            )),
        ]);
        let seeds = vec![
            TargetLabel::new("root//project/common:vendor"),
            TargetLabel::new("root//project/common:common"),
        ];
        let normalized = normalize_owner_targets_to_rust(&diff, seeds);
        assert_eq!(normalized.len(), 1);
        assert_eq!(normalized[0].as_str(), "root//project/common:common");
    }

    #[test]
    fn test_normalize_owner_targets_maps_vendor_to_rust_binary() {
        let diff = Targets::new(vec![
            TargetsEntry::Target(BuckTarget::testing(
                "vendor",
                "root//project/aardvark-dns",
                "filegroup",
            )),
            TargetsEntry::Target(BuckTarget::testing(
                "aardvark-dns",
                "root//project/aardvark-dns",
                "rust_binary",
            )),
        ]);
        let seeds = vec![TargetLabel::new("root//project/aardvark-dns:vendor")];
        let normalized = normalize_owner_targets_to_rust(&diff, seeds);
        assert_eq!(normalized.len(), 1);
        assert_eq!(
            normalized[0].as_str(),
            "root//project/aardvark-dns:aardvark-dns"
        );
    }
}
