use super::select_volume_by_selector;
use crate::{
    commands::{AsyncCliCommand, app::util::AppIdentOpts},
    config::WasmerEnv,
};

/// Enable (or disable) the S3 endpoint for an app's volumes.
///
/// Enabling S3 provisions per-volume S3 credentials; retrieve them with
/// `wasmer app volume credentials` and rotate them with `wasmer app volume
/// rotate-secrets`.
#[derive(clap::Parser, Debug)]
pub struct CmdAppVolumesEnableS3 {
    #[clap(flatten)]
    pub env: WasmerEnv,

    #[clap(flatten)]
    pub ident: AppIdentOpts,

    /// Only change this volume, by name, mount path, or volume id.
    /// Without it, every volume of the app is affected.
    #[clap(long)]
    pub volume: Option<String>,

    /// Disable S3 instead of enabling it.
    #[clap(long)]
    pub disable: bool,
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppVolumesEnableS3 {
    type Output = ();

    async fn run_async(self) -> Result<Self::Output, anyhow::Error> {
        let client = self.env.client()?;
        let (_ident, app) = self.ident.load_app(&client).await?;

        let volumes = super::list_volumes(&client, &app.owner.global_name, &app.name).await?;

        let selected: Vec<&_> = match &self.volume {
            Some(sel) => vec![select_volume_by_selector(&app.name, &volumes, sel)?],
            None => volumes.iter().collect(),
        };

        if selected.is_empty() {
            eprintln!("App {} has no volumes.", app.name);
            return Ok(());
        }

        let enable = !self.disable;
        let mut changed = 0usize;
        let mut failures = Vec::new();
        for volume in selected {
            // Skip volumes already in the desired state.
            if volume.s3_enabled == enable {
                eprintln!(
                    "S3 already {} for volume {}",
                    if enable { "enabled" } else { "disabled" },
                    volume.label()
                );
                continue;
            }

            match wasmer_backend_api::query::update_volume_s3_enabled(
                &client,
                volume.id.clone(),
                enable,
            )
            .await
            {
                Ok(payload) if payload.success => {
                    changed += 1;
                    eprintln!(
                        "{} S3 for volume {}",
                        if enable { "Enabled" } else { "Disabled" },
                        volume.label()
                    );
                }
                Ok(_) => {
                    eprintln!(
                        "Failed to update S3 for volume {}: backend reported failure",
                        volume.label()
                    );
                    failures.push(volume.label());
                }
                Err(err) => {
                    eprintln!("Failed to update S3 for volume {}: {err:#}", volume.label());
                    failures.push(volume.label());
                }
            }
        }

        if !failures.is_empty() {
            anyhow::bail!(
                "Failed to update S3 for {} volume(s): {}",
                failures.len(),
                failures.join(", ")
            );
        }

        if enable && changed > 0 {
            eprintln!("Retrieve the S3 credentials with `wasmer app volume credentials`.");
        }

        Ok(())
    }
}
