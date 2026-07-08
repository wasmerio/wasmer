use super::{ItemFormatOpts, S3CredentialRecord, render_s3_credentials};
use crate::{
    commands::{AsyncCliCommand, app::util::AppIdentOpts},
    config::WasmerEnv,
};

/// Rotate the secrets linked to volumes of an app.
#[derive(clap::Parser, Debug)]
pub struct CmdAppVolumesRotateSecrets {
    #[clap(flatten)]
    pub env: WasmerEnv,

    #[clap(flatten)]
    pub ident: AppIdentOpts,

    #[clap(flatten)]
    pub fmt: ItemFormatOpts,

    /// Only rotate this volume's credentials, by name, mount path, or volume id.
    /// Without it, every S3-enabled volume of the app is rotated.
    #[clap(long)]
    pub volume: Option<String>,
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppVolumesRotateSecrets {
    type Output = ();

    async fn run_async(self) -> Result<Self::Output, anyhow::Error> {
        let client = self.env.client()?;
        let (_ident, app) = self.ident.load_app(&client).await?;

        // rotateS3Credentials operates per AppVolume, so resolve the app's
        // volumes and rotate each one.
        let volumes =
            super::super::list_volumes(&client, &app.owner.global_name, &app.name).await?;

        let selected: Vec<&_> = match &self.volume {
            Some(sel) => vec![super::super::select_volume_by_selector(
                &app.name, &volumes, sel,
            )?],
            // Rotate every S3-enabled volume; volumes without S3 have no S3
            // credentials to rotate.
            None => volumes.iter().filter(|v| v.s3_enabled).collect(),
        };

        if selected.is_empty() {
            // Hint on stderr, but still render below so json/yaml emit `[]`.
            eprintln!(
                "App {} has no S3-enabled volumes to rotate secrets for.",
                app.name
            );
        }

        // Rotate every selected volume, recording failures instead of aborting
        // so that a single failing volume does not leave the rest un-rotated
        // and unreported.
        let mut records = Vec::new();
        let mut failures = Vec::new();
        for volume in selected {
            let result =
                wasmer_backend_api::query::rotate_s3_credentials(&client, volume.id.clone()).await;

            match result {
                Ok(payload) if payload.success => {
                    // Per-volume status on stderr, so the rotated credentials on
                    // stdout stay pipeable.
                    eprintln!("Rotated S3 credentials for volume {}", volume.label());
                    records.push(S3CredentialRecord {
                        app_name: app.name.clone(),
                        volume: volume.mount_path.clone(),
                        access_key: payload.access_key,
                        secret_key: payload.secret_key,
                        endpoint: payload.endpoint,
                    });
                }
                Ok(_) => {
                    eprintln!(
                        "Failed to rotate S3 credentials for volume {}",
                        volume.label()
                    );
                    failures.push(volume.label());
                }
                Err(err) => {
                    eprintln!(
                        "Failed to rotate S3 credentials for volume {}: {err:#}",
                        volume.label()
                    );
                    failures.push(volume.label());
                }
            }
        }

        // Emit the rotated credentials as a single document; json/yaml stay one
        // array/list (empty `[]` when nothing rotated).
        println!("{}", render_s3_credentials(self.fmt.format, &records));

        if !failures.is_empty() {
            anyhow::bail!(
                "Failed to rotate S3 credentials for {} volume(s): {}",
                failures.len(),
                failures.join(", ")
            );
        }

        Ok(())
    }
}
