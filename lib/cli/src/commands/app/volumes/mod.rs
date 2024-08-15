use crate::commands::AsyncCliCommand;

pub mod list;
pub mod s3_credentials;

/// App volume management.
#[derive(Debug, clap::Parser)]
pub enum CmdAppVolumes {
    S3Credentials(s3_credentials::CmdAppS3Credentials),
    List(list::CmdAppVolumesList),
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppVolumes {
    type Output = ();

    async fn run_async(self) -> Result<Self::Output, anyhow::Error> {
        match self {
            Self::S3Credentials(c) => {
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
