use std::path::{Path, PathBuf};

use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, PartialEq, Eq, Clone, Debug)]
pub struct EdgeConfig {
    pub version: u32,
    /// Token used for ssh access.
    pub ssh_token: Option<String>,
    /// Token used for network access.
    pub network_token: Option<String>,
}

impl EdgeConfig {
    pub const VERSION: u32 = 1;

    pub fn from_slice(data: &[u8]) -> Result<Self, anyhow::Error> {
        let data_str = std::str::from_utf8(data)?;
        let value: toml::Value = toml::from_str(data_str).context("failed to parse config TOML")?;

        let version = value
            .get("version")
            .and_then(|v| v.as_integer())
            .context("invalid client config: no 'version' key found")?;

        if version != Self::VERSION as i64 {
            bail!(
                "Invalid client config: unknown config version '{}'",
                version
            );
        }

        let config = toml::from_str(data_str)?;
        Ok(config)
    }
}

impl Default for EdgeConfig {
    fn default() -> Self {
        Self {
            ssh_token: None,
            network_token: None,
            version: 1,
        }
    }
}

const CONFIG_FILE_NAME: &str = "deploy_client.toml";
const CONFIG_PATH_ENV_VAR: &str = "DEPLOY_CLIENT_CONFIG_PATH";

pub struct LoadedEdgeConfig {
    pub config: EdgeConfig,
    pub path: PathBuf,
}

impl LoadedEdgeConfig {
    pub fn set_ssh_token(&mut self, token: String) -> Result<(), anyhow::Error> {
        self.config.ssh_token = Some(token);
        self.save()?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn set_network_token(&mut self, token: String) -> Result<(), anyhow::Error> {
        self.config.network_token = Some(token);
        self.save()?;
        Ok(())
    }

    pub fn save(&self) -> Result<(), anyhow::Error> {
        let data = toml::to_string(&self.config)?;
        std::fs::write(&self.path, data)
            .with_context(|| format!("failed to write config to '{}'", self.path.display()))?;
        Ok(())
    }
}

pub fn default_config_path() -> Result<PathBuf, anyhow::Error> {
    if let Some(var) = std::env::var_os(CONFIG_PATH_ENV_VAR) {
        Ok(var.into())
    } else {
        // TODO: use dirs crate to determine the correct path.
        // (this also depends on general wasmer config moving there.)

        #[allow(deprecated)]
        let home = std::env::home_dir().context("failed to get home directory")?;
        let path = home.join(".wasmer").join(CONFIG_FILE_NAME);
        Ok(path)
    }
}

pub fn load_config(custom_path: Option<PathBuf>) -> Result<LoadedEdgeConfig, anyhow::Error> {
    let default_path = default_config_path()?;

    let path = if let Some(p) = custom_path {
        Some(p)
    } else if default_path.is_file() {
        Some(default_path.clone())
    } else {
        None
    };

    if let Some(path) = path {
        if path.is_file() {
            match try_load_config(&path) {
                Ok(config) => {
                    return Ok(LoadedEdgeConfig { config, path });
                }
                Err(err) => {
                    eprintln!(
                        "WARNING: failed to load config file at '{}': {}",
                        path.display(),
                        err
                    );
                }
            }
        }
    }

    Ok(LoadedEdgeConfig {
        config: EdgeConfig::default(),
        path: default_path,
    })
}

fn try_load_config(path: &Path) -> Result<EdgeConfig, anyhow::Error> {
    let data = std::fs::read(path)
        .with_context(|| format!("failed to read config file at '{}'", path.display()))?;
    let config = EdgeConfig::from_slice(&data)
        .with_context(|| format!("failed to parse config file at '{}'", path.display()))?;
    Ok(config)
}
