mod login;
mod logout;
mod whoami;

pub use login::*;
pub use whoami::*;

use super::AsyncCliCommand;

/// Manage your .
#[derive(clap::Subcommand, Debug)]
pub enum CmdAuth {
    Login(login::Login),
    Logout(logout::Logout),
    Whoami(whoami::Whoami),
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAuth {
    type Output = ();

    async fn run_async(self) -> Result<Self::Output, anyhow::Error> {
        match self {
            CmdAuth::Login(l) => l.run_async().await,
            CmdAuth::Logout(l) => l.run_async().await,
            CmdAuth::Whoami(w) => w.run_async().await,
        }
    }
}
