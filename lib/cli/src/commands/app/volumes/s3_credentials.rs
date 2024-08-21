//! Get information about an edge app.

use wasmer_api::types::DeployApp;

use super::super::util::AppIdentOpts;

use crate::{commands::AsyncCliCommand, config::WasmerEnv, opts::ItemFormatOpts};

/// Retrieve S3 access credentials for the volumes of an app.
///
/// The S3 credentials can be used to access volumes with any S3 client,
/// for example rclone.
///
/// Note that --rclone-print will output an rclone configuration snippet.
#[derive(clap::Parser, Debug)]
pub struct CmdAppS3Credentials {
    #[clap(flatten)]
    pub env: WasmerEnv,

    #[clap(flatten)]
    pub fmt: ItemFormatOpts,

    #[clap(flatten)]
    pub ident: AppIdentOpts,

    /// Print rclone configuration.
    ///
    /// The output can be added the rclone configuration file, which is
    /// usually located at `~/.config/rclone/rclone.conf`.
    #[clap(long)]
    pub rclone_print: bool,
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppS3Credentials {
    type Output = DeployApp;

    async fn run_async(self) -> Result<DeployApp, anyhow::Error> {
        let client = self.env.client()?;
        let (_ident, app) = self.ident.load_app(&client).await?;

        let creds =
            wasmer_api::query::get_app_s3_credentials(&client, app.id.clone().into_inner()).await?;

        if self.rclone_print {
            let rclone_config = format!(
                r#"
[edge-{app_name}]
type = s3
provider = Other
acl = private
access_key_id = {access_key}
secret_access_key = {secret_key}
endpoint = {endpoint}

    "#,
                app_name = app.name,
                access_key = creds.access_key,
                secret_key = creds.secret_key,
                endpoint = creds.url,
            );

            println!("{}", rclone_config);
        } else {
            println!("S3 credentials for app {}:\n", app.name);
            println!("  S3 URL: {}", creds.url);
            println!("  Access key: {}", creds.access_key);
            println!("  Secret key: {}", creds.secret_key);
            println!();
            println!("Hint: use --rclone-print to generate configuration for the rclone S3 client");
            println!("Consult the app volumes documentation for more information.");
            println!();
        }

        Ok(app)
    }
}
