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
    fn run(self) -> Result<(), anyhow::Error>;
}

pub trait AsyncCliCommand: Send + Sync + 'static {
    fn run_async(self) -> futures::future::BoxFuture<'static, Result<(), anyhow::Error>>;
}

impl<C: AsyncCliCommand> CliCommand for C {
    fn run(self) -> Result<(), anyhow::Error> {
        tokio::runtime::Runtime::new()?.block_on(self.run_async())
    }
}
