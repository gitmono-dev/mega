//! Guest filesystem pressure helpers for long-lived Orion VMs.

use std::path::Path;

/// Default warn threshold (percent used). Overridable via `ORION_DISK_WARN_PCT`.
const DEFAULT_WARN_PCT: u64 = 85;
/// Default reject-new-builds threshold. Overridable via `ORION_DISK_REJECT_PCT`.
const DEFAULT_REJECT_PCT: u64 = 92;
/// Default critical threshold (still reject; prefer only heartbeats). `ORION_DISK_CRIT_PCT`.
const DEFAULT_CRIT_PCT: u64 = 98;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskPressure {
    Ok { used_pct: u64 },
    Warn { used_pct: u64 },
    Reject { used_pct: u64 },
    Critical { used_pct: u64 },
}

impl DiskPressure {
    pub fn used_pct(self) -> u64 {
        match self {
            Self::Ok { used_pct }
            | Self::Warn { used_pct }
            | Self::Reject { used_pct }
            | Self::Critical { used_pct } => used_pct,
        }
    }

    pub fn should_reject_builds(self) -> bool {
        matches!(self, Self::Reject { .. } | Self::Critical { .. })
    }
}

fn env_pct(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(|v| v.min(100))
        .unwrap_or(default)
}

/// Return used percentage of the filesystem containing `path` (0–100).
pub fn disk_used_pct(path: impl AsRef<Path>) -> Option<u64> {
    let path = path.as_ref();
    let output = std::process::Command::new("df")
        .args(["-P", &path.to_string_lossy()])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Filesystem 1024-blocks Used Available Capacity Mounted on
    let line = stdout.lines().nth(1)?;
    let capacity = line.split_whitespace().nth(4)?;
    capacity.trim_end_matches('%').parse().ok()
}

pub fn assess_root_disk() -> DiskPressure {
    let used = disk_used_pct("/").unwrap_or(0);
    let warn = env_pct("ORION_DISK_WARN_PCT", DEFAULT_WARN_PCT);
    let reject = env_pct("ORION_DISK_REJECT_PCT", DEFAULT_REJECT_PCT);
    let crit = env_pct("ORION_DISK_CRIT_PCT", DEFAULT_CRIT_PCT);

    if used >= crit {
        DiskPressure::Critical { used_pct: used }
    } else if used >= reject {
        DiskPressure::Reject { used_pct: used }
    } else if used >= warn {
        DiskPressure::Warn { used_pct: used }
    } else {
        DiskPressure::Ok { used_pct: used }
    }
}

/// Best-effort orphan Antares overlay prune when under disk pressure.
pub async fn reclaim_under_pressure() {
    match crate::antares::prune_orphan_overlay_dirs().await {
        Ok((upper, cl)) => {
            if upper + cl > 0 {
                tracing::warn!(
                    upper_removed = upper,
                    cl_removed = cl,
                    "Pruned Antares overlay dirs due to disk pressure"
                );
            }
        }
        Err(e) => tracing::warn!("Disk-pressure overlay prune failed: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assess_root_disk_returns_a_variant() {
        let _ = assess_root_disk();
    }
}
