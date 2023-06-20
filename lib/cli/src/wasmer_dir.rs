use std::path::{Path, PathBuf};

use anyhow::{Context, Error};
use once_cell::sync::Lazy;
use url::Url;
use wasmer_registry::WasmerConfig;

/// Command-line flags for determining `$WASMER_DIR` and interactions with the
/// registry.
#[derive(Debug, Clone, PartialEq, clap::Parser)]
pub struct WasmerDir {
    /// Set Wasmer's home directory
    #[clap(long, env = "WASMER_DIR", default_value = WASMER_DIR.as_os_str())]
    wasmer_dir: PathBuf,
    /// The registry to fetch packages from (inferred from the environment by
    /// default)
    #[clap(long, env = "WASMER_REGISTRY")]
    registry: Option<Registry>,
    /// The API token to use when communicating with the registry (inferred from
    /// the environment by default)
    #[clap(long, env = "WASMER_TOKEN")]
    token: Option<String>,
}

impl WasmerDir {
    pub(crate) fn new(
        wasmer_dir: PathBuf,
        registry: Option<Registry>,
        token: Option<String>,
    ) -> Self {
        WasmerDir {
            wasmer_dir,
            registry,
            token,
        }
    }

    /// Get the GraphQL endpoint used to query the registry.
    pub fn registry_endpoint(&self) -> Result<Url, Error> {
        if let Some(registry) = &self.registry {
            return registry.graphql_endpoint();
        }

        let config = self.config()?;
        let url = config.registry.get_current_registry().parse()?;

        Ok(url)
    }

    /// Load the current Wasmer config.
    pub fn config(&self) -> Result<WasmerConfig, Error> {
        WasmerConfig::from_file(self.dir())
            .map_err(Error::msg)
            .with_context(|| {
                format!(
                    "Unable to load the config from the \"{}\" directory",
                    self.dir().display()
                )
            })
    }

    /// The directory all Wasmer artifacts are stored in.
    pub fn dir(&self) -> &Path {
        &self.wasmer_dir
    }

    /// The API token for the active registry.
    pub fn token(&self) -> Option<String> {
        let config = self.config().ok()?;
        let login = config.registry.current_login()?;
        Some(login.token.clone())
    }
}

impl Default for WasmerDir {
    fn default() -> Self {
        Self {
            wasmer_dir: WASMER_DIR.clone(),
            registry: None,
            token: None,
        }
    }
}

/// The default value for `$WASMER_DIR`.
pub static WASMER_DIR: Lazy<PathBuf> =
    Lazy::new(|| match wasmer_registry::WasmerConfig::get_wasmer_dir() {
        Ok(path) => path,
        Err(e) => {
            if let Some(install_prefix) = std::env::var_os("WASMER_INSTALL_PREFIX") {
                PathBuf::from(install_prefix)
            } else {
                panic!("Unable to determine the wasmer dir: {e}");
            }
        }
    });

/// A registry as specified by the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registry(String);

impl Registry {
    /// Get the [`Registry`]'s string representation.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// Get the GraphQL endpoint for this [`Registry`].
    pub fn graphql_endpoint(&self) -> Result<Url, Error> {
        let url = wasmer_registry::format_graphql(self.as_str()).parse()?;
        Ok(url)
    }
}

impl From<String> for Registry {
    fn from(value: String) -> Self {
        Registry(value)
    }
}

impl From<&str> for Registry {
    fn from(value: &str) -> Self {
        Registry(value.to_string())
    }
}
