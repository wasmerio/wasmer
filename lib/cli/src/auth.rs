use std::path::PathBuf;

use anyhow::Context;
use url::Url;
use wasmer_registry::{wasmer_env::WasmerEnv, WasmerConfig};
use wasmer_wasix::Authentication;

/// Get an [`Authentication`] mechanism that is based on tokens from the user's
/// config file.
pub(crate) fn from_wasmer_env(env: &WasmerEnv) -> impl Authentication + Send + Sync + 'static {
    let default_registry_and_token = match (env.registry_endpoint().ok(), env.token()) {
        (Some(registry), Some(token)) => Some((registry, token)),
        _ => None,
    };

    ConfigAuthentication {
        wasmer_dir: env.dir().to_path_buf(),
        default_registry_and_token,
    }
}

#[derive(Debug, Clone, PartialEq)]
struct ConfigAuthentication {
    wasmer_dir: PathBuf,
    default_registry_and_token: Option<(Url, String)>,
}

impl Authentication for ConfigAuthentication {
    fn get_token(&self, url: &str) -> Result<Option<String>, anyhow::Error> {
        let config = WasmerConfig::from_file(&self.wasmer_dir).map_err(anyhow::Error::msg)?;

        let mut url = Url::parse(url).context("Unable to parse the URL")?;
        url.set_path("");
        let registry_url = wasmer_registry::format_graphql(url.as_str());

        // Check if the user probably passed in --token because we may be
        // talking to their default registry.
        if let Some((default_registry, token)) = &self.default_registry_and_token {
            if registry_url == default_registry.as_str() {
                return Ok(Some(token.clone()));
            }
        }

        // Otherwise, we'll need to iterate through all known tokens.
        if let Some(token) = config.registry.get_login_token_for_registry(&registry_url) {
            return Ok(Some(token));
        }

        Ok(None)
    }
}
