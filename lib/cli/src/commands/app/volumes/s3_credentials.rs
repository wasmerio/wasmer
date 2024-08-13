//! Get information about an edge app.

use wasmer_api::types::DeployApp;

use super::super::util::AppIdentOpts;

use crate::{commands::AsyncCliCommand, config::WasmerEnv, opts::ItemFormatOpts};

/// Retrieve S3 access credentials for the volumes of an app.
#[derive(clap::Parser, Debug)]
pub struct CmdAppS3Credentials {
    #[clap(flatten)]
    pub env: WasmerEnv,

    #[clap(flatten)]
    pub fmt: ItemFormatOpts,

    #[clap(flatten)]
    pub ident: AppIdentOpts,
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppS3Credentials {
    type Output = DeployApp;

    async fn run_async(self) -> Result<DeployApp, anyhow::Error> {
        let client = self.env.client()?;
        let (_ident, app) = self.ident.load_app(&client).await?;

        let creds =
            wasmer_api::query::get_app_s3_credentials(&client, app.id.clone().into_inner()).await?;

        println!("S3 credentials for app {}:\n", app.name);
        println!("  S3 URL: https://{}", creds.domain);
        println!("  Access key: {}", creds.access_key);
        println!("  Secret key: {}", creds.secret_key);
        println!();
        println!("Consult the app volumes documentation for more information.");

        Ok(app)
    }
}
