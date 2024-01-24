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

impl CmdAppGet {
    async fn run(self) -> Result<(), anyhow::Error> {
        let app = self.get_app().await?;
        println!("{}", self.fmt.format.render(&app));
        Ok(())
    }

    pub async fn get_app(&self) -> Result<DeployApp, anyhow::Error> {
        let client = self.api.client()?;
        let (_ident, app) = self.ident.load_app(&client).await?;
        Ok(app)
    }
}

impl AsyncCliCommand for CmdAppGet {
    fn run_async(self) -> futures::future::BoxFuture<'static, Result<(), anyhow::Error>> {
        Box::pin(self.run())
    }
}
