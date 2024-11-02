use crate::{
    commands::AsyncCliCommand,
    config::{WasmerConfig, WasmerEnv, DEFAULT_PROD_REGISTRY},
};
use colored::Colorize;
use is_terminal::IsTerminal;

/// Subcommand for log in a user into Wasmer (using a browser or provided a token)
#[derive(Debug, Clone, clap::Parser)]
pub struct Logout {
    #[clap(flatten)]
    env: WasmerEnv,

    /// Do not prompt for user input.
    #[clap(long, default_value_t = !std::io::stdin().is_terminal())]
    pub non_interactive: bool,

    /// Whether or not to revoke the associated token
    #[clap(long)]
    pub revoke_token: bool,
}

#[async_trait::async_trait]
impl AsyncCliCommand for Logout {
    type Output = ();

    async fn run_async(self) -> Result<Self::Output, anyhow::Error> {
        let registry = self.env.registry_endpoint()?.to_string();
        let host_str = self
            .env
            .registry_public_url()
            .map_err(|_| anyhow::anyhow!("No registry not specified!"))?
            .host_str()
            .unwrap()
            .bold();

        let client = self
            .env
            .client()
            .map_err(|_| anyhow::anyhow!("Not logged into registry {host_str}"))?;

        let user = wasmer_backend_api::query::current_user(&client)
            .await
            .map_err(|e| anyhow::anyhow!("Not logged into registry {host_str}: {e}"))?
            .ok_or_else(|| anyhow::anyhow!("Not logged into registry {host_str}"))?;

        let theme = dialoguer::theme::ColorfulTheme::default();
        let prompt = dialoguer::Confirm::with_theme(&theme).with_prompt(format!(
            "Log user {} out of registry {host_str}?",
            user.username
        ));

        if prompt.interact()? || self.non_interactive {
            let mut config = self.env.config()?;
            let token = config
                .registry
                .get_login_token_for_registry(&registry)
                .unwrap();
            config.registry.remove_registry(&registry);
            if config.registry.is_current_registry(&registry) {
                if config.registry.tokens.is_empty() {
                    _ = config
                        .registry
                        .set_current_registry(DEFAULT_PROD_REGISTRY)
                        .await;
                } else {
                    let new_reg = config.registry.tokens[0].registry.clone();
                    _ = config.registry.set_current_registry(&new_reg).await;
                }
            }
            let path = WasmerConfig::get_file_location(self.env.dir());
            config.save(path)?;

            // Read it again..
            //
            let config = self.env.config()?;
            if config
                .registry
                .get_login_token_for_registry(&registry)
                .is_none()
            {
                println!(
                    "User {} correctly logged out of registry {host_str}",
                    user.username.bold()
                );

                let should_revoke = self.revoke_token || {
                    let theme = dialoguer::theme::ColorfulTheme::default();
                    dialoguer::Confirm::with_theme(&theme)
                        .with_prompt("Revoke token?")
                        .interact()?
                };

                if should_revoke {
                    wasmer_backend_api::query::revoke_token(&client, token).await?;
                    println!(
                        "Token for user {} in registry {host_str} correctly revoked",
                        user.username.bold()
                    );
                }
            } else {
                anyhow::bail!("Something went wrong! User is not logged out.")
            }
        } else {
            println!("No action taken.");
        }

        Ok(())
    }
}
