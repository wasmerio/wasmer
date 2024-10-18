use crate::commands::AsyncCliCommand;

pub mod get;
pub mod list;
pub mod logs;

/// App volume management.
#[derive(Debug, clap::Parser)]
pub enum CmdAppDeployment {
    List(list::CmdAppDeploymentList),
    Get(get::CmdAppDeploymentGet),
    Logs(logs::CmdAppDeploymentLogs),
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppDeployment {
    type Output = ();

    async fn run_async(self) -> Result<Self::Output, anyhow::Error> {
        match self {
            Self::List(c) => c.run_async().await,
            Self::Get(c) => c.run_async().await,
            Self::Logs(c) => c.run_async().await,
        }
    }
}
