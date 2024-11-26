//! Get logs for an app deployment.

use std::io::Write;

use anyhow::Context;
use futures::stream::TryStreamExt;

use crate::{commands::AsyncCliCommand, config::WasmerEnv, opts::ItemFormatOpts};

/// Get logs for an app deployment.
#[derive(clap::Parser, Debug)]
pub struct CmdAppDeploymentLogs {
    #[clap(flatten)]
    fmt: ItemFormatOpts,

    #[clap(flatten)]
    env: WasmerEnv,

    /// ID of the deployment.
    id: String,
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppDeploymentLogs {
    type Output = ();

    async fn run_async(mut self) -> Result<(), anyhow::Error> {
        let client = self.env.client()?;
        let item = wasmer_backend_api::query::app_deployment(&client, self.id).await?;

        let url = item
            .log_url
            .context("This deployment does not have logs available")?;

        let mut writer = std::io::BufWriter::new(std::io::stdout());

        let mut stream = reqwest::Client::new()
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .bytes_stream();

        while let Some(chunk) = stream.try_next().await? {
            writer.write_all(&chunk)?;
            writer.flush()?;
        }

        Ok(())
    }
}
