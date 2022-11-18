use clap::Parser;
use dialoguer::Input;

/// Subcommand for listing packages
#[derive(Debug, Clone, Parser)]
pub struct Login {
    /// Registry to log into (default: wapm.io)
    #[clap(long, default_value = "wapm.io")]
    pub registry: String,
    /// Login token
    #[clap(name = "TOKEN")]
    pub token: Option<String>,
}

impl Login {
    fn get_token_or_ask_user(&self) -> Result<String, std::io::Error> {
        match self.token.as_ref() {
            Some(s) => Ok(s.clone()),
            None => {
                let registry_host = url::Url::parse(&wasmer_registry::format_graphql(
                    &self.registry,
                ))
                .map_err(|e| {
                    std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("Invalid registry for login {}: {e}", self.registry),
                    )
                })?;
                let registry_host = registry_host.host_str().ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("Invalid registry for login {}: no host", self.registry),
                    )
                })?;
                Input::new()
                    .with_prompt(&format!(
                        "Please paste the login token from https://{}/me:\"",
                        registry_host
                    ))
                    .interact_text()
            }
        }
    }

    /// execute [List]
    pub fn execute(&self) -> Result<(), anyhow::Error> {
        let token = self.get_token_or_ask_user()?;
        wasmer_registry::login::login_and_save_token(&self.registry, &token)
            .map_err(|e| anyhow::anyhow!("{e}"))
    }
}
