use clap::Parser;
use colored::Colorize;

use crate::config::WasmerEnv;

use super::AsyncCliCommand;

#[derive(Debug, Parser)]
/// The options for the `wasmer whoami` subcommand
pub struct Whoami {
    #[clap(flatten)]
    env: WasmerEnv,
}

#[async_trait::async_trait]
impl AsyncCliCommand for Whoami {
    type Output = ();

    /// Execute `wasmer whoami`
    async fn run_async(self) -> Result<Self::Output, anyhow::Error> {
        let client = self.env.client_unauthennticated()?;
        let host_str = self.env.registry_public_url()?.host_str().unwrap().bold();
        let user = wasmer_backend_api::query::current_user(&client)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Not logged in registry {host_str}"))?;
        println!(
            "Logged into registry {host_str} as user {}",
            user.username.bold()
        );
        Ok(())
    }
}
