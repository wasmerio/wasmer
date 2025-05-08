use crate::commands::AsyncCliCommand;

pub mod list;

/// App volume management.
#[derive(Debug, clap::Parser)]
pub enum CmdAppDatabase {
    //  Credentials(credentials::CmdAppDatabaseCredentials),
    List(list::CmdAppDatabaseList),
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppDatabase {
    type Output = ();

    async fn run_async(self) -> Result<Self::Output, anyhow::Error> {
        match self {
            Self::List(c) => {
                c.run_async().await?;
                Ok(())
            }
        }
    }
}
