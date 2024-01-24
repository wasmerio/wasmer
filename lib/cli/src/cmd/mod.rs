pub mod app;
pub mod connect;
pub mod deploy;
pub mod namespace;
pub mod ssh;

#[derive(clap::Subcommand, Debug)]
pub enum SubCmd {
    #[clap(subcommand)]
    App(app::CmdApp),
    #[clap(subcommand)]
    Namespace(namespace::CmdNamespace),
    Deploy(deploy::CmdDeploy),
    Ssh(ssh::CmdSsh),
    Connect(connect::CmdConnect),
}

impl CliCommand for SubCmd {
    type Output = ();

    fn run(self) -> Result<(), anyhow::Error> {
        match self {
            Self::Ssh(cmd) => cmd.run(),
            Self::App(cmd) => cmd.run(),
            Self::Namespace(cmd) => cmd.run(),
            Self::Deploy(cmd) => cmd.run(),
            Self::Connect(cmd) => cmd.run(),
        }
    }
}

pub trait CliCommand {
    type Output;

    fn run(self) -> Result<(), anyhow::Error>;
}

#[async_trait::async_trait]
pub trait AsyncCliCommand: Send + Sync {
    type Output: Send + Sync;

    async fn run_async(self) -> Result<Self::Output, anyhow::Error>;
}

impl<O: Send + Sync, C: AsyncCliCommand<Output = O>> CliCommand for C {
    type Output = O;

    fn run(self) -> Result<(), anyhow::Error> {
        tokio::runtime::Runtime::new()?.block_on(AsyncCliCommand::run_async(self))?;
        Ok(())
    }
}
