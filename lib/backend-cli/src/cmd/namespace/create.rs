use crate::{cmd::AsyncCliCommand, ApiOpts, ItemFormatOpts};

/// Create a new namespace.
#[derive(clap::Parser, Debug)]
pub struct CmdNamespaceCreate {
    #[clap(flatten)]
    fmt: ItemFormatOpts,
    #[clap(flatten)]
    api: ApiOpts,

    /// Description of the namespace.
    #[clap(long)]
    description: Option<String>,

    /// Name of the namespace.
    name: String,
}

impl CmdNamespaceCreate {
    async fn run(self) -> Result<(), anyhow::Error> {
        let client = self.api.client()?;

        let vars = wasmer_api::types::CreateNamespaceVars {
            name: self.name.clone(),
            description: self.description.clone(),
        };
        let namespace = wasmer_api::query::create_namespace(&client, vars).await?;

        println!("{}", self.fmt.format.render(&namespace));

        Ok(())
    }
}

impl AsyncCliCommand for CmdNamespaceCreate {
    fn run_async(self) -> futures::future::BoxFuture<'static, Result<(), anyhow::Error>> {
        Box::pin(self.run())
    }
}
