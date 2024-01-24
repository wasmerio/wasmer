use wasmer_api::types::DeployApp;

use crate::{cmd::AsyncCliCommand, ApiOpts, ItemFormatOpts};

use super::util::AppIdentOpts;

/// Show an app.
#[derive(clap::Parser, Debug)]
pub struct CmdAppGet {
    #[clap(flatten)]
    pub api: ApiOpts,
    #[clap(flatten)]
    pub fmt: ItemFormatOpts,

    #[clap(flatten)]
    pub ident: AppIdentOpts,
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppGet {
    type Output = DeployApp;

    async fn run_async(self) -> Result<DeployApp, anyhow::Error> {
        let client = self.api.client()?;
        let (_ident, app) = self.ident.load_app(&client).await?;
        Ok(app)
    }
}
