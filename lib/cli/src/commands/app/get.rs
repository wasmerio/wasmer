//! Get information about an edge app.

use wasmer_backend_api::types::DeployApp;

use super::util::AppIdentOpts;

use crate::{
    commands::AsyncCliCommand, config::WasmerEnv, opts::ItemFormatOpts, utils::render::ItemFormat,
};

/// Retrieve detailed informations about an app
#[derive(clap::Parser, Debug)]
pub struct CmdAppGet {
    #[clap(flatten)]
    pub env: WasmerEnv,

    #[clap(flatten)]
    pub fmt: ItemFormatOpts,

    #[clap(flatten)]
    pub ident: AppIdentOpts,
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppGet {
    type Output = DeployApp;

    async fn run_async(self) -> Result<DeployApp, anyhow::Error> {
        let client = self.env.client()?;
        let (_ident, app) = self.ident.load_app(&client).await?;

        println!(
            "{}",
            self.fmt.get_with_default(ItemFormat::Yaml).render(&app)
        );

        Ok(app)
    }
}
