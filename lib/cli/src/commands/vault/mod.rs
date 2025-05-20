pub mod create;

use crate::commands::AsyncCliCommand;

/// Manage namespaces.
#[derive(clap::Subcommand, Debug)]
pub enum CmdVault {
    Create(create::CmdVaultCreate),
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdVault {
    type Output = ();

    async fn run_async(self) -> Result<(), anyhow::Error> {
        match self {
            CmdVault::Create(cmd) => cmd.run_async().await,
        }
    }
}
