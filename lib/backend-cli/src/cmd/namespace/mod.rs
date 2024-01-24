pub mod create;
pub mod get;
pub mod list;

use crate::cmd::AsyncCliCommand;

/// Manage namespaces.
#[derive(clap::Subcommand, Debug)]
pub enum CmdNamespace {
    Get(get::CmdNamespaceGet),
    List(list::CmdNamespaceList),
}

impl AsyncCliCommand for CmdNamespace {
    fn run_async(self) -> futures::future::BoxFuture<'static, Result<(), anyhow::Error>> {
        match self {
            CmdNamespace::List(cmd) => cmd.run_async(),
            CmdNamespace::Get(cmd) => cmd.run_async(),
        }
    }
}
