use std::path::{Path, PathBuf};

use crate::WasmerConfig;
use anyhow::{Context, Error};
use url::Url;

/// Command-line flags for determining the local "Wasmer Environment".
///
/// This is where you access `$WASMER_DIR`, the `$WASMER_DIR/wasmer.toml` config
/// file, and specify the current registry.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "clap", derive(clap::Parser))]
pub struct WasmerEnv {
    /// Set Wasmer's home directory
    #[cfg_attr(feature = "clap", clap(long, env = "WASMER_DIR", default_value = WASMER_DIR.as_os_str()))]
    wasmer_dir: PathBuf,
    /// The registry to fetch packages from (inferred from the environment by
    /// default)
    #[cfg_attr(feature = "clap", clap(long, env = "WASMER_REGISTRY"))]
    registry: Option<Registry>,
    /// The directory cached artefacts are saved to.
    #[cfg_attr(feature = "clap", clap(long, env = "WASMER_CACHE_DIR"))]
    cache_dir: Option<PathBuf>,
    /// The API token to use when communicating with the registry (inferred from
    /// the environment by default)
    #[cfg_attr(feature = "clap", clap(long, env = "WASMER_TOKEN"))]
    token: Option<String>,
}

impl WasmerEnv {
    pub fn new(
        wasmer_dir: PathBuf,
        registry: Option<Registry>,
        token: Option<String>,
        cache_dir: Option<PathBuf>,
    ) -> Self {
        WasmerEnv {
            wasmer_dir,
            registry,
            token,
            cache_dir,
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

    /// The directory all cached artifacts should be saved to.
    pub fn cache_dir(&self) -> PathBuf {
        match &self.cache_dir {
            Some(cache_dir) => cache_dir.clone(),
            None => self.dir().join("cache"),
        }
    }

    /// The API token for the active registry.
    pub fn token(&self) -> Option<String> {
        let config = self.config().ok()?;
        let login = config.registry.current_login()?;
        Some(login.token.clone())
    }
}

impl Default for WasmerEnv {
    fn default() -> Self {
        Self {
            wasmer_dir: WASMER_DIR.clone(),
            registry: None,
            token: None,
            cache_dir: None,
        }
    }
}

lazy_static::lazy_static! {
    /// The default value for `$WASMER_DIR`.
    pub static ref WASMER_DIR: PathBuf = match crate::WasmerConfig::get_wasmer_dir() {
        Ok(path) => path,
        Err(e) => {
            if let Some(install_prefix) = option_env!("WASMER_INSTALL_PREFIX") {
                return PathBuf::from(install_prefix);
            }

            panic!("Unable to determine the wasmer dir: {e}");
        }
    };
}

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
        let url = crate::format_graphql(self.as_str()).parse()?;
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
