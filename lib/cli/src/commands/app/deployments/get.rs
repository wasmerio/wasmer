//! Get deployments for an app.

use crate::{
    commands::AsyncCliCommand, config::WasmerEnv, opts::ItemFormatOpts, utils::render::ItemFormat,
};

/// Get the volumes of an app.
#[derive(clap::Parser, Debug)]
pub struct CmdAppDeploymentGet {
    #[clap(flatten)]
    fmt: ItemFormatOpts,

    #[clap(flatten)]
    env: WasmerEnv,

    /// ID of the deployment.
    id: String,
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppDeploymentGet {
    type Output = ();

    async fn run_async(mut self) -> Result<(), anyhow::Error> {
        let client = self.env.client()?;
        let item = wasmer_api::query::app_deployment(&client, self.id).await?;

        // The below is a bit hacky - the default is YAML, but this is not a good
        // default for deployments, so we switch to table.
        if self.fmt.format == ItemFormat::Yaml {
            self.fmt.format = ItemFormat::Table;
        }

        println!("{}", self.fmt.format.render(&item));
        Ok(())
    }
}
