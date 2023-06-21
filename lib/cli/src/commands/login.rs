use std::path::PathBuf;

use clap::Parser;
#[cfg(not(test))]
use dialoguer::Input;

use wasmer_registry::wasmer_env::{Registry, WasmerEnv, WASMER_DIR};

/// Subcommand for listing packages
#[derive(Debug, Clone, Parser)]
pub struct Login {
    // Note: This is essentially a copy of WasmerEnv except the token is
    // accepted as a main argument instead of via --token.
    /// Set Wasmer's home directory
    #[clap(long, env = "WASMER_DIR", default_value = WASMER_DIR.as_os_str())]
    pub wasmer_dir: PathBuf,
    /// The registry to fetch packages from (inferred from the environment by
    /// default)
    #[clap(long, env = "WASMER_REGISTRY")]
    pub registry: Option<Registry>,
    /// The API token to use when communicating with the registry (inferred from
    /// the environment by default)
    pub token: Option<String>,
    /// The directory cached artefacts are saved to.
    #[clap(long, env = "WASMER_CACHE_DIR")]
    cache_dir: Option<PathBuf>,
}

impl Login {
    fn get_token_or_ask_user(&self, env: &WasmerEnv) -> Result<String, anyhow::Error> {
        if let Some(token) = &self.token {
            return Ok(token.clone());
        }

        let registry_host = env.registry_endpoint()?;
        let registry_tld = tldextract::TldExtractor::new(tldextract::TldOption::default())
            .extract(registry_host.as_str())
            .map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Invalid registry for login {}: {e}", registry_host),
                )
            })?;
        let login_prompt = match (
            registry_tld.domain.as_deref(),
            registry_tld.suffix.as_deref(),
        ) {
            (Some(d), Some(s)) => {
                format!("Please paste the login token from https://{d}.{s}/settings/access-tokens")
            }
            _ => "Please paste the login token".to_string(),
        };
        #[cfg(test)]
        {
            Ok(login_prompt)
        }
        #[cfg(not(test))]
        {
            let token = Input::new().with_prompt(&login_prompt).interact_text()?;
            Ok(token)
        }
    }

    fn wasmer_env(&self) -> WasmerEnv {
        WasmerEnv::new(
            self.wasmer_dir.clone(),
            self.registry.clone(),
            self.token.clone(),
            self.cache_dir.clone(),
        )
    }

    /// execute [List]
    pub fn execute(&self) -> Result<(), anyhow::Error> {
        let env = self.wasmer_env();
        let token = self.get_token_or_ask_user(&env)?;

        let registry = env.registry_endpoint()?;
        match wasmer_registry::login::login_and_save_token(env.dir(), registry.as_str(), &token)? {
            Some(s) => println!("Login for Wasmer user {:?} saved", s),
            None => println!(
                "Error: no user found on registry {:?} with token {:?}. Token saved regardless.",
                registry, token
            ),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn interactive_login() {
        let temp = TempDir::new().unwrap();
        let login = Login {
            registry: Some("wasmer.wtf".into()),
            wasmer_dir: temp.path().to_path_buf(),
            token: None,
            cache_dir: None,
        };
        let env = login.wasmer_env();

        let token = login.get_token_or_ask_user(&env).unwrap();

        assert_eq!(
            token,
            "Please paste the login token from https://wasmer.wtf/settings/access-tokens"
        );
    }

    #[test]
    fn login_with_token() {
        let temp = TempDir::new().unwrap();
        let login = Login {
            registry: Some("wasmer.wtf".into()),
            wasmer_dir: temp.path().to_path_buf(),
            token: Some("abc".to_string()),
            cache_dir: None,
        };
        let env = login.wasmer_env();

        let token = login.get_token_or_ask_user(&env).unwrap();

        assert_eq!(token, "abc");
    }

    #[test]
    fn in_sync_with_wasmer_env() {
        let wasmer_env = WasmerEnv::command();
        let login = Login::command();

        // All options except --token should be the same
        let wasmer_env_opts: Vec<_> = wasmer_env
            .get_opts()
            .filter(|arg| arg.get_id() != "token")
            .collect();
        let login_opts: Vec<_> = login.get_opts().collect();

        assert_eq!(wasmer_env_opts, login_opts);

        // The token argument should have the same message, even if it is an
        // argument rather than a --flag.
        let wasmer_env_token_help = wasmer_env
            .get_opts()
            .find(|arg| arg.get_id() == "token")
            .unwrap()
            .get_help()
            .unwrap()
            .to_string();
        let login_token_help = login
            .get_positionals()
            .find(|arg| arg.get_id() == "token")
            .unwrap()
            .get_help()
            .unwrap()
            .to_string();
        assert_eq!(wasmer_env_token_help, login_token_help);
    }
}
