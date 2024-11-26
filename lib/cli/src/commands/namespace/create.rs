use crate::{commands::AsyncCliCommand, config::WasmerEnv, opts::ItemFormatOpts};

/// Create a new namespace.
#[derive(clap::Parser, Debug)]
pub struct CmdNamespaceCreate {
    #[clap(flatten)]
    fmt: ItemFormatOpts,

    #[clap(flatten)]
    env: WasmerEnv,

    /// Description of the namespace.
    #[clap(long)]
    description: Option<String>,

    /// Name of the namespace.
    name: String,
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdNamespaceCreate {
    type Output = ();

    async fn run_async(self) -> Result<(), anyhow::Error> {
        let client = self.env.client()?;

        let vars = wasmer_backend_api::types::CreateNamespaceVars {
            name: self.name.clone(),
            description: self.description.clone(),
        };
        let namespace = wasmer_backend_api::query::create_namespace(&client, vars).await?;

        println!("{}", self.fmt.get().render(&namespace));

        Ok(())
    }
}
