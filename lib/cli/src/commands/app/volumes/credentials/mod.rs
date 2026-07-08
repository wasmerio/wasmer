pub(super) mod rotate_secrets;

use crate::{
    commands::{AsyncCliCommand, app::util::AppIdentOpts},
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

        // S3 credentials are per-volume; fetch every volume and print the
        // credentials of those that have S3 enabled.
        let volumes = super::list_volumes(&client, &app.owner.global_name, &app.name).await?;

        let with_creds: Vec<_> = volumes
            .iter()
            .filter_map(|volume| volume.s3.as_ref().map(|s3| (volume, s3)))
            .collect();

        if with_creds.is_empty() {
            // Hint on stderr, but still render below so json/yaml emit `[]`.
            eprintln!(
                "App {} has no S3-enabled volumes with credentials. \
                 Enable S3 with `wasmer app volume enable-s3`.",
                app.name
            );
        }

        let records: Vec<S3CredentialRecord> = with_creds
            .into_iter()
            .map(|(volume, s3)| S3CredentialRecord {
                app_name: app.name.clone(),
                volume: volume.mount_path.clone(),
                access_key: s3.access_key.clone(),
                secret_key: s3.secret_key.clone(),
                endpoint: s3.endpoint.clone(),
            })
            .collect();

        println!("{}", render_s3_credentials(self.fmt.format, &records));

        Ok(())
    }
}

/// One volume's S3 credentials, ready to be rendered.
#[derive(Debug, serde::Serialize)]
pub(crate) struct S3CredentialRecord {
    pub app_name: String,
    /// The mount path the credentials belong to.
    pub volume: String,
    pub access_key: String,
    pub secret_key: String,
    pub endpoint: String,
}

/// Render a set of volumes' S3 credentials in the requested format.
///
/// For `json`/`yaml` the records are emitted as a single array/list so that the
/// output is one valid document no matter how many volumes there are; for
/// `rclone`/`table` each record is rendered in turn (concatenated sections, one
/// table with a row per volume). The returned string is ready to print as-is.
pub(crate) fn render_s3_credentials(
    format: CredsItemFormat,
    records: &[S3CredentialRecord],
) -> String {
    match format {
        CredsItemFormat::Rclone => records.iter().map(render_rclone_section).collect(),
        CredsItemFormat::Json => serde_json::to_string_pretty(records).unwrap(),
        CredsItemFormat::Yaml => serde_yaml::to_string(records).unwrap(),
        CredsItemFormat::Table => {
            let mut table = comfy_table::Table::new();
            table.add_row(vec![
                "App name",
                "Volume",
                "Access key",
                "Secret key",
                "Endpoint",
            ]);
            for record in records {
                table.add_row(vec![
                    record.app_name.as_str(),
                    record.volume.as_str(),
                    record.access_key.as_str(),
                    record.secret_key.as_str(),
                    record.endpoint.as_str(),
                ]);
            }
            table.to_string()
        }
    }
}

/// Render one volume's credentials as an rclone configuration section.
fn render_rclone_section(record: &S3CredentialRecord) -> String {
    let S3CredentialRecord {
        app_name,
        volume,
        access_key,
        secret_key,
        endpoint,
    } = record;
    let section = format!(
        "edge-{app_name}-{}",
        volume.trim_start_matches('/').replace('/', "-")
    );
    format!(
        r#"
[{section}]
# rclone configuration for volume {volume} of {app_name}
type = s3
provider = Other
acl = private
access_key_id = {access_key}
secret_access_key = {secret_key}
endpoint = {endpoint}

"#
    )
}

/*
 * The following is a copy of [`crate::opts::ItemFormatOpts`] with an
 * additional formatting - rclone - that only makes sense in this context.
 */

/// The possible formats to output the credentials in.
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
