use super::WasmerConfig;
use anyhow::{Context, Error};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use url::Url;
use wasmer_backend_api::WasmerClient;

pub static DEFAULT_WASMER_CLI_USER_AGENT: LazyLock<String> =
    LazyLock::new(|| format!("WasmerCLI-v{}", env!("CARGO_PKG_VERSION")));

/// Command-line flags for determining the local "Wasmer Environment".
///
/// This is where you access `$WASMER_DIR`, the `$WASMER_DIR/wasmer.toml` config
/// file, and specify the current registry.
#[derive(Debug, Clone, PartialEq, clap::Parser)]
pub struct WasmerEnv {
    /// Set Wasmer's home directory
    #[clap(long, env = "WASMER_DIR", default_value = super::DEFAULT_WASMER_DIR.as_os_str())]
    wasmer_dir: PathBuf,

    /// The directory cached artefacts are saved to.
    #[clap(long, env = "WASMER_CACHE_DIR", default_value = super::DEFAULT_WASMER_CACHE_DIR.as_os_str())]
    pub(crate) cache_dir: PathBuf,

    /// The registry to fetch packages from (inferred from the environment by
    /// default)
    #[clap(long, env = "WASMER_REGISTRY")]
    pub(crate) registry: Option<UserRegistry>,

    /// The API token to use when communicating with the registry (inferred from
    /// the environment by default)
    #[clap(long, env = "WASMER_TOKEN")]
    token: Option<String>,
}

impl WasmerEnv {
    pub fn new(
        wasmer_dir: PathBuf,
        cache_dir: PathBuf,
        token: Option<String>,
        registry: Option<UserRegistry>,
    ) -> Self {
        WasmerEnv {
            wasmer_dir,
            registry,
            token,
            cache_dir,
        }
    }

    /// Get the "public" url of the current registry (e.g. "https://wasmer.io" instead of
    /// "https://registry.wasmer.io/graphql").
    pub fn registry_public_url(&self) -> Result<Url, Error> {
        let mut url = self.registry_endpoint()?;
        url.set_path("");

        let mut domain = url.host_str().context("url has no host")?.to_string();
        if domain.starts_with("registry.") {
            domain = domain.strip_prefix("registry.").unwrap().to_string();
        }

        url.set_host(Some(&domain))
            .context("could not derive registry public url")?;

        Ok(url)
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

    /// Returns the proxy specified in wasmer config if present
    pub fn proxy(&self) -> Result<Option<reqwest::Proxy>, Error> {
        self.config()?
            .proxy
            .url
            .as_ref()
            .map(reqwest::Proxy::all)
            .transpose()
            .map_err(Into::into)
    }

    /// The directory all Wasmer artifacts are stored in.
    pub fn dir(&self) -> &Path {
        &self.wasmer_dir
    }

    /// The directory all cached artifacts should be saved to.
    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    /// Retrieve the specified token.
    ///
    /// NOTE: In contrast to [`Self::token`], this will not fall back to loading
    /// the token from the confg file.
    #[allow(unused)]
    pub fn get_token_opt(&self) -> Option<&str> {
        self.token.as_deref()
    }

    /// The API token for the active registry.
    pub fn token(&self) -> Option<String> {
        if let Some(token) = &self.token {
            return Some(token.clone());
        }

        // Fall back to the config file

        let config = self.config().ok()?;
        let registry_endpoint = self.registry_endpoint().ok()?;
        config
            .registry
            .get_login_token_for_registry(registry_endpoint.as_str())
    }

    pub fn client_unauthennticated(&self) -> Result<WasmerClient, anyhow::Error> {
        let registry_url = self.registry_endpoint()?;

        let proxy = self.proxy()?;

        let client = wasmer_backend_api::WasmerClient::new_with_proxy(
            registry_url,
            &DEFAULT_WASMER_CLI_USER_AGENT,
            proxy,
        )?;

        let client = if let Some(token) = self.token() {
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

impl Default for WasmerEnv {
    fn default() -> Self {
        Self {
            wasmer_dir: super::DEFAULT_WASMER_DIR.clone(),
            cache_dir: super::DEFAULT_WASMER_CACHE_DIR.clone(),
            registry: None,
            token: None,
        }
    }
}

/// A registry as specified by the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserRegistry(String);

impl UserRegistry {
    /// Get the [`Registry`]'s string representation.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// Get the GraphQL endpoint for this [`Registry`].
    pub fn graphql_endpoint(&self) -> Result<Url, Error> {
        let url = super::format_graphql(self.as_str()).parse()?;
        Ok(url)
    }
}

impl From<String> for UserRegistry {
    fn from(value: String) -> Self {
        UserRegistry(value)
    }
}

impl From<&str> for UserRegistry {
    fn from(value: &str) -> Self {
        UserRegistry(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    const WASMER_TOML: &str = r#"
    telemetry_enabled = false
    update_notifications_enabled = false

    [registry]
    active_registry = "https://registry.wasmer.io/graphql"

    [[registry.tokens]]
    registry = "https://registry.wasmer.wtf/graphql"
    token = "dev-token"

    [[registry.tokens]]
    registry = "https://registry.wasmer.io/graphql"
    token = "prod-token"

    [[registry.tokens]]
    registry = "http://localhost:11/graphql"
    token = "invalid"
    "#;

    #[test]
    fn load_defaults_from_config() {
        let temp = TempDir::new().unwrap();
        std::fs::write(temp.path().join("wasmer.toml"), WASMER_TOML).unwrap();

        let env = WasmerEnv {
            wasmer_dir: temp.path().to_path_buf(),
            registry: None,
            cache_dir: temp.path().join("cache").to_path_buf(),
            token: None,
        };

        assert_eq!(
            env.registry_endpoint().unwrap().as_str(),
            "https://registry.wasmer.io/graphql"
        );
        assert_eq!(env.token().unwrap(), "prod-token");
        assert_eq!(env.cache_dir(), temp.path().join("cache"));
    }

    #[test]
    fn override_token() {
        let temp = TempDir::new().unwrap();
        std::fs::write(temp.path().join("wasmer.toml"), WASMER_TOML).unwrap();

        let env = WasmerEnv {
            wasmer_dir: temp.path().to_path_buf(),
            registry: None,
            cache_dir: temp.path().join("cache").to_path_buf(),
            token: Some("asdf".to_string()),
        };

        assert_eq!(
            env.registry_endpoint().unwrap().as_str(),
            "https://registry.wasmer.io/graphql"
        );
        assert_eq!(env.token().unwrap(), "asdf");
        assert_eq!(env.cache_dir(), temp.path().join("cache"));
    }

    #[test]
    fn override_registry() {
        let temp = TempDir::new().unwrap();
        std::fs::write(temp.path().join("wasmer.toml"), WASMER_TOML).unwrap();
        let env = WasmerEnv {
            wasmer_dir: temp.path().to_path_buf(),
            registry: Some(UserRegistry::from("wasmer.wtf")),
            cache_dir: temp.path().join("cache").to_path_buf(),
            token: None,
        };

        assert_eq!(
            env.registry_endpoint().unwrap().as_str(),
            "https://registry.wasmer.wtf/graphql"
        );
        assert_eq!(env.token().unwrap(), "dev-token");
        assert_eq!(env.cache_dir(), temp.path().join("cache"));
    }

    #[test]
    fn override_registry_and_token() {
        let temp = TempDir::new().unwrap();
        std::fs::write(temp.path().join("wasmer.toml"), WASMER_TOML).unwrap();

        let env = WasmerEnv {
            wasmer_dir: temp.path().to_path_buf(),
            registry: Some(UserRegistry::from("wasmer.wtf")),
            cache_dir: temp.path().join("cache").to_path_buf(),
            token: Some("asdf".to_string()),
        };

        assert_eq!(
            env.registry_endpoint().unwrap().as_str(),
            "https://registry.wasmer.wtf/graphql"
        );
        assert_eq!(env.token().unwrap(), "asdf");
        assert_eq!(env.cache_dir(), temp.path().join("cache"));
    }

    #[test]
    fn override_cache_dir() {
        let temp = TempDir::new().unwrap();
        std::fs::write(temp.path().join("wasmer.toml"), WASMER_TOML).unwrap();
        let expected_cache_dir = temp.path().join("some-other-cache");

        let env = WasmerEnv {
            wasmer_dir: temp.path().to_path_buf(),
            registry: None,
            cache_dir: expected_cache_dir.clone(),
            token: None,
        };

        assert_eq!(
            env.registry_endpoint().unwrap().as_str(),
            "https://registry.wasmer.io/graphql"
        );
        assert_eq!(env.token().unwrap(), "prod-token");
        assert_eq!(env.cache_dir(), expected_cache_dir);
    }

    #[test]
    fn registries_have_public_url() {
        let temp = TempDir::new().unwrap();
        std::fs::write(temp.path().join("wasmer.toml"), WASMER_TOML).unwrap();

        let inputs = [
            ("https://wasmer.io/", "https://registry.wasmer.io/graphql"),
            ("https://wasmer.wtf/", "https://registry.wasmer.wtf/graphql"),
            ("https://wasmer.wtf/", "https://registry.wasmer.wtf/graphql"),
            (
                "https://wasmer.wtf/",
                "https://registry.wasmer.wtf/something/else",
            ),
            ("https://wasmer.wtf/", "https://wasmer.wtf/graphql"),
            ("https://wasmer.wtf/", "https://wasmer.wtf/graphql"),
            ("http://localhost:8000/", "http://localhost:8000/graphql"),
            ("http://localhost:8000/", "http://localhost:8000/graphql"),
        ];

        for (want, input) in inputs {
            let env = WasmerEnv {
                wasmer_dir: temp.path().to_path_buf(),
                registry: Some(UserRegistry::from(input)),
                cache_dir: temp.path().join("cache").to_path_buf(),
                token: None,
            };

            assert_eq!(want, &env.registry_public_url().unwrap().to_string())
        }
    }
}
