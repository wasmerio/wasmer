use anyhow::Context;

use crate::{commands::AsyncCliCommand, config::WasmerEnv, opts::ItemFormatOpts};

/// Show a namespace.
#[derive(clap::Parser, Debug)]
pub struct CmdNamespaceGet {
    #[clap(flatten)]
    fmt: ItemFormatOpts,

    #[clap(flatten)]
    env: WasmerEnv,

    /// Name of the namespace.
    name: String,
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdNamespaceGet {
    type Output = ();

    async fn run_async(self) -> Result<(), anyhow::Error> {
        let client = self.env.client()?;

        let namespace = wasmer_backend_api::query::get_namespace(&client, self.name)
            .await?
            .context("namespace not found")?;

        println!("{}", self.fmt.get().render(&namespace));

        Ok(())
    }
}
