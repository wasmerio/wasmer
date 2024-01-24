use crate::cmd::AsyncCliCommand;

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

impl AsyncCliCommand for CmdAppVersion {
    fn run_async(self) -> futures::future::BoxFuture<'static, Result<(), anyhow::Error>> {
        match self {
            Self::Get(cmd) => cmd.run_async(),
            Self::List(cmd) => cmd.run_async(),
            Self::Activate(cmd) => cmd.run_async(),
        }
    }
}
