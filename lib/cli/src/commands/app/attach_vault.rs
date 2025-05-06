//! Attach a vault to the specified app

use anyhow::Context;
use super::util::AppIdentOpts;
use crate::{commands::AsyncCliCommand, config::WasmerEnv};

/// Attach a vault to the specified app
#[derive(clap::Parser, Debug)]
pub struct CmdAppAttachVault {
    #[clap(flatten)]
    env: WasmerEnv,

    /// Vault ID to attach
    vault_id: String,

    #[clap(flatten)]
    ident: AppIdentOpts,


}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppAttachVault {
    type Output = ();

    async fn run_async(self) -> Result<(), anyhow::Error> {
        let client = self.env.client()?;
        let (_ident, app) = self.ident.load_app(&client).await?;

        let resp_app = wasmer_backend_api::query::attach_vault(
            &self.env.client()?,
            self.vault_id.into(),
            app.id,
        ).await.context("Attaching vault to app")?;
        
        println!("Attached vault to app {}", resp_app.global_name);
        Ok(())
    }
}
