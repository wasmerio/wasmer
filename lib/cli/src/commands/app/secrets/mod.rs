use crate::commands::AsyncCliCommand;

pub mod create;
pub mod delete;
pub mod export;
pub mod import;
pub mod list;
pub mod reveal;
pub mod update;
mod utils;

/// Manage and reveal secrets related to Edge apps.
#[derive(Debug, clap::Parser)]
pub enum CmdAppSecrets {
    Create(create::CmdAppSecretsCreate),
    Delete(delete::CmdAppSecretsDelete),
    Export(export::CmdAppSecretsExport),
    Import(import::CmdAppSecretsImport),
    Reveal(reveal::CmdAppSecretsReveal),
    List(list::CmdAppSecretsList),
    Update(update::CmdAppSecretsUpdate),
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppSecrets {
    type Output = ();

    async fn run_async(self) -> Result<Self::Output, anyhow::Error> {
        match self {
            CmdAppSecrets::Create(c) => {
                c.run_async().await?;
                Ok(())
            }
            CmdAppSecrets::Delete(c) => {
                c.run_async().await?;
                Ok(())
            }
            CmdAppSecrets::Export(c) => {
                c.run_async().await?;
                Ok(())
            }
            CmdAppSecrets::Import(c) => {
                c.run_async().await?;
                Ok(())
            }
            CmdAppSecrets::Reveal(c) => {
                c.run_async().await?;
                Ok(())
            }
            CmdAppSecrets::List(c) => {
                c.run_async().await?;
                Ok(())
            }

            CmdAppSecrets::Update(c) => {
                c.run_async().await?;
                Ok(())
            }
        }
    }
}
