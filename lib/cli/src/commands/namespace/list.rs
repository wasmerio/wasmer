use crate::{
    commands::AsyncCliCommand,
    opts::{ApiOpts, ListFormatOpts},
};

/// List namespaces.
#[derive(clap::Parser, Debug)]
pub struct CmdNamespaceList {
    #[clap(flatten)]
    fmt: ListFormatOpts,
    #[clap(flatten)]
    api: ApiOpts,
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdNamespaceList {
    type Output = ();

    async fn run_async(self) -> Result<(), anyhow::Error> {
        let client = self.api.client()?;

        let namespaces = wasmer_api::query::user_namespaces(&client).await?;

        println!("{}", self.fmt.format.render(&namespaces));

        Ok(())
    }
}
