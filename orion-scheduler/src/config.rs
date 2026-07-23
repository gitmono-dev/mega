use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result};
use serde::Deserialize;
use tokio::sync::RwLock;

/// Runner environment URLs passed inline via webhook (no targets map lookup).
#[derive(Debug, Clone, Deserialize)]
pub struct TargetConfig {
    pub server_ws: String,
    pub scorpio_base_url: String,
    pub scorpio_lfs_url: String,
}

/// Default VM image parameters used when webhook omits image_* fields.
#[derive(Debug, Clone, Deserialize)]
pub struct DefaultImageConfig {
    pub image_path: String,
    pub image_digest: String,
    pub image_disk_gb: u32,
    pub image_cpus: u32,
    pub image_memory_mb: u32,
}

impl Default for DefaultImageConfig {
    fn default() -> Self {
        Self {
            image_path: "~/.local/share/qlean/images/debian-13-buck2/debian-13-buck2.qcow2"
                .to_string(),
            image_digest: "sha256:753c28888c9d30fe4baef55c1d1dfa9a39431595eca940b7ad85d78d84f3d7a5"
                .to_string(),
            image_disk_gb: 50,
            image_cpus: 8,
            image_memory_mb: 16000,
        }
    }
}

/// Scheduler configuration loaded from JSON file.
#[derive(Debug, Clone)]
pub struct Config {
    log_dir: String,
    orion_source_dir: String,
    orion_binary_path: String,
    ssh_public_key_path: String,
    default_image: DefaultImageConfig,
    /// Max concurrent VMs (by domain). `None` = unlimited.
    max_vms: Option<usize>,
}

/// Expand a leading `~` or `~/` to `$HOME`. Other paths are returned unchanged.
pub fn expand_tilde(path: impl AsRef<str>) -> PathBuf {
    let path = path.as_ref();
    if path == "~" {
        return std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(path));
    }
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home).join(rest);
    }
    PathBuf::from(path)
}

impl Config {
    #[cfg(test)]
    pub fn new(
        log_dir: String,
        orion_source_dir: String,
        orion_binary_path: String,
        ssh_public_key_path: String,
        default_image: DefaultImageConfig,
    ) -> Self {
        Self {
            log_dir,
            orion_source_dir,
            orion_binary_path,
            ssh_public_key_path,
            default_image,
            max_vms: None,
        }
    }

    pub async fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let abs = absolutize(path);
        let content = tokio::fs::read_to_string(path).await.with_context(|| {
            format!(
                "failed to read config file at {} \
                 (set CONFIG_PATH or run from a directory containing target_config.json)",
                abs.display()
            )
        })?;
        let parsed: ConfigFile = serde_json::from_str(&content)
            .with_context(|| format!("failed to parse JSON config at {}", abs.display()))?;

        let orion_source_dir = parsed.orion_source_dir.ok_or_else(|| {
            anyhow::anyhow!(
                "missing required field 'orion_source_dir' in config file {}",
                abs.display()
            )
        })?;
        let orion_binary_path = parsed.orion_binary_path.ok_or_else(|| {
            anyhow::anyhow!(
                "missing required field 'orion_binary_path' in config file {}",
                abs.display()
            )
        })?;
        let ssh_public_key_path = parsed.ssh_public_key_path.ok_or_else(|| {
            anyhow::anyhow!(
                "missing required field 'ssh_public_key_path' in config file {}",
                abs.display()
            )
        })?;

        Ok(Config {
            log_dir: parsed
                .log_dir
                .unwrap_or_else(|| "/var/log/orion-scheduler".to_string()),
            orion_source_dir,
            orion_binary_path,
            ssh_public_key_path,
            default_image: parsed.default_image.unwrap_or_default(),
            max_vms: parsed.max_vms,
        })
    }

    pub fn log_dir(&self) -> &str {
        &self.log_dir
    }

    pub fn orion_source_dir(&self) -> &str {
        &self.orion_source_dir
    }

    pub fn orion_binary_path(&self) -> &str {
        &self.orion_binary_path
    }

    pub fn ssh_public_key_path(&self) -> &str {
        &self.ssh_public_key_path
    }

    pub fn default_image(&self) -> &DefaultImageConfig {
        &self.default_image
    }

    pub fn max_vms(&self) -> Option<usize> {
        self.max_vms
    }
}

pub fn default_config_path() -> Option<PathBuf> {
    default_config_candidates()
        .into_iter()
        .find(|p| p.is_file())
}

pub fn default_config_candidates() -> Vec<PathBuf> {
    const FILE_NAME: &str = "target_config.json";
    let mut out: Vec<PathBuf> = Vec::new();

    out.push(PathBuf::from(FILE_NAME));

    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        out.push(dir.join(FILE_NAME));
    }

    out.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(FILE_NAME));

    out
}

fn absolutize(path: &Path) -> PathBuf {
    if let Ok(canonical) = std::fs::canonicalize(path) {
        return canonical;
    }
    if path.is_absolute() {
        return path.to_path_buf();
    }
    match std::env::current_dir() {
        Ok(cwd) => cwd.join(path),
        Err(_) => path.to_path_buf(),
    }
}

#[derive(Debug, Deserialize)]
struct ConfigFile {
    #[serde(default)]
    log_dir: Option<String>,
    #[serde(default)]
    orion_source_dir: Option<String>,
    #[serde(default)]
    orion_binary_path: Option<String>,
    #[serde(default)]
    ssh_public_key_path: Option<String>,
    #[serde(default)]
    default_image: Option<DefaultImageConfig>,
    #[serde(default)]
    max_vms: Option<usize>,
}

pub type SharedConfig = Arc<RwLock<Config>>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_tilde_home_prefix() {
        let home = std::env::var("HOME").expect("HOME must be set in test env");
        let p = expand_tilde("~/.local/share/qlean/images/debian-13-buck2.qcow2");
        assert_eq!(
            p,
            std::path::PathBuf::from(home).join(".local/share/qlean/images/debian-13-buck2.qcow2")
        );
    }

    #[test]
    fn expand_tilde_absolute_unchanged() {
        let abs = "/home/orion/image.qcow2";
        assert_eq!(expand_tilde(abs), std::path::PathBuf::from(abs));
    }

    #[test]
    fn default_image_has_expected_defaults() {
        let d = DefaultImageConfig::default();
        assert_eq!(d.image_disk_gb, 50);
        assert_eq!(d.image_cpus, 8);
        assert_eq!(d.image_memory_mb, 16000);
        assert!(d.image_digest.starts_with("sha256:"));
    }
}
