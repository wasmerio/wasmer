use std::path::PathBuf;

use anyhow::Context;
use wasmer_api::WasmerClient;
use wasmer_registry::WasmerConfig;

#[derive(clap::Parser, Debug, Clone, Default)]
pub struct ApiOpts {
    /// The API token for the backend.
    /// Inferred from the environment by default.
    #[clap(long, env = "WASMER_TOKEN")]
    pub token: Option<String>,

    /// The backend URL.
    /// May be either a full URL to a GraphQL endpoint, or a plain URL.
    /// Plain URLs are expanded to "<registry.URL/graphql>"
    #[clap(long, env = "WASMER_REGISTRY")]
    pub registry: Option<url::Url>,

    /// The directory cached artefacts are saved to.
    #[clap(long, env = "WASMER_CACHE_DIR")]
    cache_dir: Option<PathBuf>,
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

    /// Get the backend client.
    ///
    /// If a token is provided in the options, the token will be used.
    /// Otherwise the client will be anonymous and unauthenticated.
    ///
    /// Use [`Self::client_authenticated`] to get an error if no token is
    /// available.
    pub fn client(&self) -> Result<WasmerClient, anyhow::Error> {
        let login = self.build_login()?;

        let client = wasmer_api::WasmerClient::new(login.url, "edge-cli")?;

        let client = if let Some(token) = login.token {
            client.with_auth_token(token)
        } else {
            client
        };

        Ok(client)
    }

    /// Get the backend client that is authenticated.
    ///
    /// Returns an error if no token is available.
    pub fn client_authenticated(&self) -> Result<WasmerClient, anyhow::Error> {
        let client = self.client()?;
        if client.auth_token().is_none() {
            return Err(ApiUnauthenticatedError.into());
        }

        Ok(client)
    }
}

#[derive(thiserror::Error, Debug)]
#[error("No backend token provided - run 'wasmer login', specify --token=XXX, or set the WASMER_TOKEN env var")]
pub struct ApiUnauthenticatedError;

/// Formatting options for a single item.
#[derive(clap::Parser, Debug, Default)]
pub struct ItemFormatOpts {
    /// Output format. (json, text)
    #[clap(short = 'f', long, default_value = "yaml")]
    pub format: crate::utils::render::ItemFormat,
}

/// Formatting options for a list of items.
#[derive(clap::Parser, Debug)]
pub struct ListFormatOpts {
    /// Output format. (json, text)
    #[clap(short = 'f', long, default_value = "table")]
    pub format: crate::utils::render::ListFormat,
}
