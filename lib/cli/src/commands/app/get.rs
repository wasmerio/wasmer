//! Get information about an edge app.

use wasmer_api::types::DeployApp;

use super::util::AppIdentOpts;
use crate::{
    commands::AsyncCliCommand,
    opts::{ApiOpts, ItemFormatOpts},
};

/// Show an app.
#[derive(clap::Parser, Debug)]
pub struct CmdAppGet {
    #[clap(flatten)]
    #[allow(missing_docs)]
    pub api: ApiOpts,
    #[clap(flatten)]
    #[allow(missing_docs)]
    pub fmt: ItemFormatOpts,

    #[clap(flatten)]
    #[allow(missing_docs)]
    pub ident: AppIdentOpts,
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppGet {
    type Output = DeployApp;

    async fn run_async(self) -> Result<DeployApp, anyhow::Error> {
        let client = self.api.client()?;
        let (_ident, app) = self.ident.load_app(&client).await?;

        println!("{}", self.fmt.format.render(&app));

        Ok(app)
    }
}
