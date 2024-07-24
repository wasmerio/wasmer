use colored::Colorize;

use crate::{
    commands::AsyncCliCommand,
    config::{WasmerConfig, WasmerEnv},
};

/// Subcommand for log in a user into Wasmer (using a browser or provided a token)
#[derive(Debug, Clone, clap::Parser)]
pub struct Logout {
    #[clap(flatten)]
    env: WasmerEnv,
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

        let user = wasmer_api::query::current_user(&client)
            .await
            .map_err(|e| anyhow::anyhow!("Not logged into registry {host_str}: {e}"))?
            .ok_or_else(|| anyhow::anyhow!("Not logged into registry {host_str}"))?;

        let theme = dialoguer::theme::ColorfulTheme::default();
        let prompt = dialoguer::Confirm::with_theme(&theme).with_prompt(format!(
            "Log user {} out of registry {host_str}?",
            user.username.bold()
        ));

        if prompt.interact()? {
            let mut config = self.env.config()?;
            config.registry.remove_registry(&registry);
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
            } else {
                anyhow::bail!("Something went wrong! User is not logged out.")
            }
        } else {
            println!("No action taken.");
        }

        Ok(())
    }
}
