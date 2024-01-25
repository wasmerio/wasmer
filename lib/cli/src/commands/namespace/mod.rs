pub mod create;
pub mod get;
pub mod list;

use crate::commands::AsyncCliCommand;

/// Manage namespaces.
#[derive(clap::Subcommand, Debug)]
pub enum CmdNamespace {
    Get(get::CmdNamespaceGet),
    List(list::CmdNamespaceList),
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdNamespace {
    type Output = ();

    async fn run_async(self) -> Result<(), anyhow::Error> {
        match self {
            CmdNamespace::List(cmd) => cmd.run_async().await,
            CmdNamespace::Get(cmd) => cmd.run_async().await,
        }
    }
}
