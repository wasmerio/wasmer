use crate::{commands::AsyncCliCommand, config::WasmerEnv, opts::ListFormatOpts};

/// List namespaces.
#[derive(clap::Parser, Debug)]
pub struct CmdNamespaceList {
    #[clap(flatten)]
    fmt: ListFormatOpts,
    #[clap(flatten)]
    env: WasmerEnv,
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdNamespaceList {
    type Output = ();

    async fn run_async(self) -> Result<(), anyhow::Error> {
        let client = self.env.client()?;

        let namespaces = wasmer_backend_api::query::user_namespaces(&client).await?;

        println!("{}", self.fmt.format.render(&namespaces));

        Ok(())
    }
}
