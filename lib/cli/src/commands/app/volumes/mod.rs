use crate::commands::AsyncCliCommand;

pub mod credentials;
pub mod list;

/// App volume management.
#[derive(Debug, clap::Parser)]
pub enum CmdAppVolumes {
    Credentials(credentials::CmdAppVolumesCredentials),
    List(list::CmdAppVolumesList),
    RotateSecrets(credentials::rotate_secrets::CmdAppVolumesRotateSecrets),
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppVolumes {
    type Output = ();

    async fn run_async(self) -> Result<Self::Output, anyhow::Error> {
        match self {
            Self::Credentials(c) => {
                c.run_async().await?;
                Ok(())
            }
            Self::RotateSecrets(c) => {
                c.run_async().await?;
                Ok(())
            }
            Self::List(c) => {
                c.run_async().await?;
                Ok(())
            }
        }
    }
}
