use std::{
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use toml::Value;

use crate::{
    config::{mega_base, template::default_config_template},
    utils::get_current_bin_name,
};

#[derive(Debug, Clone, Copy)]
pub enum ConfigSource {
    Cli,
    Env,
    /// `config/config.local.toml` under the current working directory
    CwdLocal,
    /// `config/config.toml` under the current working directory
    Cwd,
    Global,
    DefaultGenerated,
}

pub struct LoadedConfig {
    pub path: PathBuf,
    pub source: ConfigSource,
}

#[derive(Debug, Default)]
pub struct ConfigInput {
    /// CLI --config
    pub cli_path: Option<PathBuf>,

    /// ENV: MEGA_CONFIG
    pub env_path: Option<PathBuf>,
}

pub struct ConfigLoader {
    input: ConfigInput,
}

impl ConfigLoader {
    pub fn new(input: ConfigInput) -> Self {
        Self { input }
    }

    /// Load config path, create default config if not exists
    pub fn load(&self) -> Result<LoadedConfig> {
        if let Some(path) = &self.input.cli_path {
            return Ok(LoadedConfig {
                path: path.clone(),
                source: ConfigSource::Cli,
            });
        }

        if let Some(path) = &self.input.env_path {
            return Ok(LoadedConfig {
                path: path.clone(),
                source: ConfigSource::Env,
            });
        }

        if let Some(loaded) = Self::cwd_config()? {
            return Ok(loaded);
        }

        if let Some(path) = Self::global_config_path()? {
            return Ok(LoadedConfig {
                path,
                source: ConfigSource::Global,
            });
        }

        let path = self.create_default_config()?;
        Ok(LoadedConfig {
            path,
            source: ConfigSource::DefaultGenerated,
        })
    }

    /// Prefer `config/config.local.toml`, then fall back to `config/config.toml`.
    fn cwd_config() -> Result<Option<LoadedConfig>> {
        let cwd = env::current_dir().context("failed to get current dir")?;

        let local = cwd.join("config/config.local.toml");
        if local.exists() {
            return Ok(Some(LoadedConfig {
                path: local,
                source: ConfigSource::CwdLocal,
            }));
        }

        let path = cwd.join("config/config.toml");
        Ok(path.exists().then_some(LoadedConfig {
            path,
            source: ConfigSource::Cwd,
        }))
    }

    fn global_config_path() -> Result<Option<PathBuf>> {
        let path = mega_base().join("etc/config.toml");
        Ok(path.exists().then_some(path))
    }

    fn create_default_config(&self) -> Result<PathBuf> {
        let base_dir = mega_base();
        let etc_dir = base_dir.join("etc");
        fs::create_dir_all(&etc_dir).with_context(|| format!("failed to create {:?}", etc_dir))?;

        let bin_name = get_current_bin_name();
        let template = default_config_template(&bin_name)
            .with_context(|| format!("no default config template for binary `{}`", bin_name))?;

        let config = Self::render_template(template, &base_dir)?;
        let config_path = etc_dir.join("config.toml");

        fs::write(&config_path, config)
            .with_context(|| format!("failed to write {:?}", config_path))?;

        eprintln!(
            "config.toml not found, created default config at {:?}",
            config_path
        );

        Ok(config_path)
    }

    fn render_template(template: &str, base_dir: &Path) -> Result<String> {
        let mut value: Value =
            toml::from_str(template).context("failed to parse default config template")?;

        value["base_dir"] = Value::String(base_dir.to_string_lossy().into());

        toml::to_string_pretty(&value).context("failed to serialize default config")
    }
}
