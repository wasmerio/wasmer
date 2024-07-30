use crate::commands::AsyncCliCommand;

pub mod list;
mod utils;

/// Informations about available Edge regioins.
#[derive(Debug, clap::Parser)]
pub enum CmdAppRegions {
    List(list::CmdAppRegionsList),
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppRegions {
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
