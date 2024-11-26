pub(super) mod rotate_secrets;

use crate::{
    commands::{app::util::AppIdentOpts, AsyncCliCommand},
    config::WasmerEnv,
};

/// Retrieve access credentials for the volumes of an app.
///
/// The credentials can be used to access volumes with any S3 client, for example rclone. Note:
/// using --format=rclone - which is the default - will output an rclone configuration snippet.
#[derive(clap::Parser, Debug)]
pub struct CmdAppVolumesCredentials {
    #[clap(flatten)]
    pub env: WasmerEnv,

    #[clap(flatten)]
    pub fmt: ItemFormatOpts,

    #[clap(flatten)]
    pub ident: AppIdentOpts,
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppVolumesCredentials {
    type Output = ();

    async fn run_async(self) -> Result<Self::Output, anyhow::Error> {
        let client = self.env.client()?;
        let (_ident, app) = self.ident.load_app(&client).await?;

        let creds =
            wasmer_backend_api::query::get_app_s3_credentials(&client, app.id.clone().into_inner())
                .await?;

        match self.fmt.format {
            CredsItemFormat::Rclone => {
                let rclone_config = format!(
                    r#"
[edge-{app_name}]
# rclone configuration for volumes of {app_name}
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
                    endpoint = creds.endpoint,
                );
                println!("{rclone_config}");
            }
            CredsItemFormat::Json => {
                let value = serde_json::json!({
                    "app_name": app.name,
                    "access_key": creds.access_key,
                    "secret_key": creds.secret_key,
                    "endpoint": creds.endpoint,
                });

                println!("{}", serde_json::to_string_pretty(&value).unwrap());
            }
            CredsItemFormat::Yaml => {
                let config = format!(
                    r#"
app_name: {app_name}
access_key: {access_key}
secret_key: {secret_key}
endpoint: {endpoint}
"#,
                    app_name = app.name,
                    access_key = creds.access_key,
                    secret_key = creds.secret_key,
                    endpoint = creds.endpoint,
                );
                println!("{config}");
            }

            CredsItemFormat::Table => {
                let mut table = comfy_table::Table::new();
                table.add_row(vec!["App name", "Access key", "Secret key", "Endpoint"]);
                table.add_row(vec![
                    app.name,
                    creds.access_key,
                    creds.secret_key,
                    creds.endpoint,
                ]);

                println!("{table}");
            }
        }

        Ok(())
    }
}

/*
 * The following is a copy of [`crate::opts::ItemFormatOpts`] with an
 * additional formatting - rclone - that only makes sense in this context.
 */

/// The possibile formats to output the credentials in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum CredsItemFormat {
    Json,
    Yaml,
    Table,
    Rclone,
}

/// Formatting options for credentials.
#[derive(clap::Parser, Debug)]
pub struct ItemFormatOpts {
    /// Output format
    #[clap(short = 'f', long, default_value = "rclone")]
    pub format: CredsItemFormat,
}
