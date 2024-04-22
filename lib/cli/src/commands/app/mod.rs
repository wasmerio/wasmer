//! Edge app commands.

pub mod create;
pub mod delete;
pub mod deploy;
pub mod get;
pub mod info;
pub mod list;
pub mod logs;
pub mod version;

mod util;

use crate::commands::AsyncCliCommand;

/// Manage Wasmer Deploy apps.
#[derive(clap::Subcommand, Debug)]
pub enum CmdApp {
    Get(get::CmdAppGet),
    Info(info::CmdAppInfo),
    List(list::CmdAppList),
    Logs(logs::CmdAppLogs),
    Create(create::CmdAppCreate),
    Delete(delete::CmdAppDelete),
    #[clap(subcommand)]
    Version(version::CmdAppVersion),
    Deploy(deploy::CmdAppDeploy),
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdApp {
    type Output = ();

    async fn run_async(self) -> Result<Self::Output, anyhow::Error> {
        match self {
            Self::Get(cmd) => {
                cmd.run_async().await?;
                Ok(())
            }
            Self::Info(cmd) => {
                cmd.run_async().await?;
                Ok(())
            }
            Self::Create(cmd) => {
                cmd.run_async().await?;
                Ok(())
            }
            Self::List(cmd) => cmd.run_async().await,
            Self::Logs(cmd) => cmd.run_async().await,
            Self::Delete(cmd) => cmd.run_async().await,
            Self::Version(cmd) => cmd.run_async().await,
            Self::Deploy(cmd) => cmd.run_async().await,
        }
    }
}
