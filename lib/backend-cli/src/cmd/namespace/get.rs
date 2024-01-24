use anyhow::Context;

use crate::{cmd::AsyncCliCommand, ApiOpts, ItemFormatOpts};

/// Show a namespace.
#[derive(clap::Parser, Debug)]
pub struct CmdNamespaceGet {
    #[clap(flatten)]
    fmt: ItemFormatOpts,
    #[clap(flatten)]
    api: ApiOpts,

    /// Name of the namespace.
    name: String,
}

impl CmdNamespaceGet {
    async fn run(self) -> Result<(), anyhow::Error> {
        let client = self.api.client()?;

        let namespace = wasmer_api::query::get_namespace(&client, self.name)
            .await?
            .context("namespace not found")?;

        println!("{}", self.fmt.format.render(&namespace));

        Ok(())
    }
}

impl AsyncCliCommand for CmdNamespaceGet {
    fn run_async(self) -> futures::future::BoxFuture<'static, Result<(), anyhow::Error>> {
        Box::pin(self.run())
    }
}
