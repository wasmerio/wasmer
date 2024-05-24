use std::path::PathBuf;

use anyhow::Context;
use wasmer_api::WasmerClient;
use wasmer_registry::WasmerConfig;

fn parse_registry_url(registry: &str) -> Result<url::Url, String> {
    if let Ok(mut url) = url::Url::parse(registry) {
        // Looks like we've got a valid URL. Let's try to use it as-is.
        if url.has_host() {
            if url.path() == "/" {
                // make sure we convert http://registry.wasmer.io/ to
                // http://registry.wasmer.io/graphql
                url.set_path("/graphql");
            }

            return Ok(url);
        }
    }

    let raw_registry = if !registry.contains("://") && !registry.contains('/') {
        if registry.contains("localhost") {
            format!("http://{registry}/graphql")
        } else {
            format!("https://registry.{registry}/graphql")
        }
    } else {
        registry.to_string()
    };

    url::Url::parse(&raw_registry).map_err(|e| e.to_string())
}

#[derive(clap::Parser, Debug, Clone, Default)]
pub struct ApiOpts {
    #[clap(long, env = "WASMER_TOKEN")]
    pub token: Option<String>,
    #[clap(long, value_parser = parse_registry_url)]
    pub registry: Option<url::Url>,
}

lazy_static::lazy_static! {
    /// The default value for `$WASMER_DIR`.
    pub static ref WASMER_DIR: PathBuf = match WasmerConfig::get_wasmer_dir() {
        Ok(path) => path,
        Err(e) => {
            if let Some(install_prefix) = option_env!("WASMER_INSTALL_PREFIX") {
                return PathBuf::from(install_prefix);
            }

            panic!("Unable to determine the wasmer dir: {e}");
        }
    };

    /// The default value for `$WASMER_DIR`.
    pub static ref WASMER_CACHE_DIR: PathBuf = WASMER_DIR.join("cache");
}

/// Command-line flags for determining the local "Wasmer Environment".
///
/// This is where you access `$WASMER_DIR`, the `$WASMER_DIR/wasmer.toml` config
/// file, and specify the current registry.
#[derive(Debug, Clone, PartialEq, clap::Parser, Default)]
pub struct WasmerEnv {
    /// Set Wasmer's home directory
    #[clap(long, env = "WASMER_DIR", default_value = WASMER_DIR.as_os_str())]
    pub wasmer_dir: PathBuf,
    /// The directory cached artefacts are saved to.
    #[clap(long, env = "WASMER_CACHE_DIR", default_value = WASMER_CACHE_DIR.as_os_str())]
    pub cache_dir: PathBuf,
}

struct Login {
    url: url::Url,
    token: Option<String>,
}

impl ApiOpts {
    const PROD_REGISTRY: &'static str = "https://registry.wasmer.io/graphql";

    fn load_config() -> Result<WasmerConfig, anyhow::Error> {
        let wasmer_dir = WasmerConfig::get_wasmer_dir()
            .map_err(|e| anyhow::anyhow!("no wasmer dir: '{e}' - did you run 'wasmer login'?"))?;

        let config = WasmerConfig::from_file(&wasmer_dir).map_err(|e| {
            anyhow::anyhow!("could not load config '{e}' - did you run wasmer login?")
        })?;

        Ok(config)
    }

    fn build_login(&self) -> Result<Login, anyhow::Error> {
        let config = Self::load_config()?;

        let login = if let Some(reg) = &self.registry {
            let token = if let Some(token) = &self.token {
                Some(token.clone())
            } else {
                config.registry.get_login_token_for_registry(reg.as_str())
            };

            Login {
                url: reg.clone(),
                token,
            }
        } else if let Some(login) = config.registry.current_login() {
            let url = login.registry.parse::<url::Url>().with_context(|| {
                format!(
                    "config file specified invalid registry url: '{}'",
                    login.registry
                )
            })?;

            let token = self.token.clone().unwrap_or(login.token.clone());

            Login {
                url,
                token: Some(token),
            }
        } else {
            let url = Self::PROD_REGISTRY.parse::<url::Url>().unwrap();
            Login {
                url,
                token: self.token.clone(),
            }
        };

        Ok(login)
    }

    pub fn client_unauthennticated(&self) -> Result<WasmerClient, anyhow::Error> {
        let login = self.build_login()?;

        let client = wasmer_api::WasmerClient::new(login.url, "edge-cli")?;

        let client = if let Some(token) = login.token {
            client.with_auth_token(token)
        } else {
            client
        };

        Ok(client)
    }

    pub fn client(&self) -> Result<WasmerClient, anyhow::Error> {
        let client = self.client_unauthennticated()?;
        if client.auth_token().is_none() {
            anyhow::bail!("no token provided - run 'wasmer login', specify --token=XXX, or set the WASMER_TOKEN env var");
        }

        Ok(client)
    }
}

/// Formatting options for a single item.
#[derive(clap::Parser, Debug, Default)]
pub struct ItemFormatOpts {
    /// Output format. (yaml, json, table)
    #[clap(short = 'f', long, default_value = "yaml")]
    pub format: crate::utils::render::ItemFormat,
}

/// Formatting options for a single item.
#[derive(clap::Parser, Debug, Default)]
pub struct ItemTableFormatOpts {
    /// Output format. (yaml, json, table)
    #[clap(short = 'f', long, default_value = "table")]
    pub format: crate::utils::render::ItemFormat,
}

/// Formatting options for a list of items.
#[derive(clap::Parser, Debug)]
pub struct ListFormatOpts {
    /// Output format. (yaml, json, table, item-table)
    #[clap(short = 'f', long, default_value = "table")]
    pub format: crate::utils::render::ListFormat,
}
