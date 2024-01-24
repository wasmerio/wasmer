use crate::{cmd::AsyncCliCommand, ApiOpts, ListFormatOpts};

/// List namespaces.
#[derive(clap::Parser, Debug)]
pub struct CmdNamespaceList {
    #[clap(flatten)]
    fmt: ListFormatOpts,
    #[clap(flatten)]
    api: ApiOpts,
}

impl CmdNamespaceList {
    async fn run(self) -> Result<(), anyhow::Error> {
        let client = self.api.client()?;

        let namespaces = wasmer_api::query::user_namespaces(&client).await?;

        println!("{}", self.fmt.format.render(&namespaces));

        Ok(())
    }
}

impl AsyncCliCommand for CmdNamespaceList {
    fn run_async(self) -> futures::future::BoxFuture<'static, Result<(), anyhow::Error>> {
        Box::pin(self.run())
    }
}
