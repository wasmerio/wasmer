//! Edge app commands.

pub mod create;
pub mod database;
pub mod delete;
pub mod deploy;
mod deployments;
pub mod get;
pub mod info;
pub mod list;
pub mod logs;
pub mod purge_cache;
pub mod regions;
pub mod secrets;
pub mod version;
pub mod volumes;

mod util;

use crate::commands::AsyncCliCommand;

/// Manage Wasmer Deploy apps.
#[derive(clap::Subcommand, Debug)]
pub enum CmdApp {
    Deploy(deploy::CmdAppDeploy),
    Create(create::CmdAppCreate),
    Get(get::CmdAppGet),
    Info(info::CmdAppInfo),
    List(list::CmdAppList),
    Logs(logs::CmdAppLogs),
    PurgeCache(purge_cache::CmdAppPurgeCache),
    Delete(delete::CmdAppDelete),
    #[clap(subcommand)]
    Version(version::CmdAppVersion),
    #[clap(subcommand, alias = "secrets")]
    Secret(secrets::CmdAppSecrets),
    #[clap(subcommand, alias = "regions")]
    Region(regions::CmdAppRegions),
    #[clap(subcommand, alias = "volumes")]
    Volume(volumes::CmdAppVolumes),
    #[clap(subcommand, alias = "databases")]
    Database(database::CmdAppDatabase),
    #[clap(subcommand, alias = "deployments")]
    Deployment(deployments::CmdAppDeployment),
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
            Self::PurgeCache(cmd) => cmd.run_async().await,
            Self::Secret(cmd) => cmd.run_async().await,
            Self::Region(cmd) => cmd.run_async().await,
            Self::Volume(cmd) => cmd.run_async().await,
            Self::Database(cmd) => cmd.run_async().await,
            Self::Deployment(cmd) => cmd.run_async().await,
        }
    }
}
