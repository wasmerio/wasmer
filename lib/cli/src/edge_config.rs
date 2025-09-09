use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

#[derive(Serialize, Deserialize, PartialEq, Eq, Clone, Debug)]
pub struct EdgeConfig {
    pub version: u32,
    /// Token used for ssh access.
    pub ssh_token: Option<String>,

    /// Map from unique app ID (da_...) to SSH token.
    #[serde(default)]
    pub ssh_app_tokens: HashMap<String, String>,

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

    /// Get a valid SSH token.
    ///
    /// Will filter out the stored token if it has expired.
    pub fn get_valid_ssh_token(&self, app_id: Option<&str>) -> Option<&str> {
        #[allow(clippy::manual_filter)]
        if let Some(app_id) = app_id {
            let token = self.ssh_app_tokens.get(app_id)?;
            if jwt_token_valid(token) {
                Some(token)
            } else {
                None
            }
        } else if let Some(token) = &self.ssh_token {
            if jwt_token_valid(token) {
                Some(token)
            } else {
                None
            }
        } else {
            None
        }
    }

    pub fn add_ssh_token(&mut self, app_id: Option<String>, token: String) {
        if let Some(app_id) = app_id {
            self.ssh_app_tokens.insert(app_id, token);
        } else {
            self.ssh_token = Some(token);
        }
    }
}

impl Default for EdgeConfig {
    fn default() -> Self {
        Self {
            ssh_token: None,
            ssh_app_tokens: HashMap::new(),
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

fn jwt_token_valid(jwt_token: &str) -> bool {
    if let Ok(expiration) = jwt_token_expiration_time(jwt_token) {
        expiration > (OffsetDateTime::now_utc() + Duration::from_secs(30))
    } else {
        false
    }
}

fn jwt_token_expiration_time(jwt_token: &str) -> Result<OffsetDateTime, anyhow::Error> {
    use base64::Engine;

    // JWT tokens have three parts separated by dots: Header.Payload.Signature
    let parts: Vec<&str> = jwt_token.split('.').collect();

    if parts.len() != 3 {
        bail!("Invalid JWT token format: expected 3 parts.");
    }

    let payload_base64 = parts[1];

    // Decode the Base64 URL-encoded payload
    let decoded_payload_bytes = base64::prelude::BASE64_URL_SAFE_NO_PAD
        .decode(payload_base64)
        .with_context(|| "Failed to base64 decode payload")?;

    let decoded_payload_str = String::from_utf8(decoded_payload_bytes)
        .with_context(|| "Failed to convert decoded bytes to UTF-8")?;

    // Parse the JSON payload
    let payload_json: serde_json::Value = serde_json::from_str(&decoded_payload_str)
        .with_context(|| "Failed to parse JSON payload")?;

    // Extract the 'exp' (expiration time) claim
    // It should be a Unix timestamp (seconds since epoch)
    let exp_timestamp = payload_json
        .get("exp")
        .context("no 'exp' field in token payload")?
        .as_i64()
        .context("exp field is not an integer")?;

    // Convert the Unix timestamp to an OffsetDateTime
    let expiration_time = OffsetDateTime::from_unix_timestamp(exp_timestamp)
        .context("Failed to convert Unix timestamp to OffsetDateTime")?;

    Ok(expiration_time)
}
