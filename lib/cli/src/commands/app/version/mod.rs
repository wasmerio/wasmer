//! Edge app version management.

use crate::commands::AsyncCliCommand;

pub mod activate;
pub mod get;
pub mod list;

/// Manage app versions.
#[derive(clap::Subcommand, Debug)]
pub enum CmdAppVersion {
    Get(get::CmdAppVersionGet),
    List(list::CmdAppVersionList),
    Activate(activate::CmdAppVersionActivate),
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppVersion {
    type Output = ();

    async fn run_async(self) -> Result<(), anyhow::Error> {
        match self {
            Self::Get(cmd) => cmd.run_async().await,
            Self::List(cmd) => cmd.run_async().await,
            Self::Activate(cmd) => cmd.run_async().await,
        }
    }
}
