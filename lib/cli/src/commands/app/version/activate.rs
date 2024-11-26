use crate::{commands::AsyncCliCommand, config::WasmerEnv, opts::ItemFormatOpts};

/// Switch the active version of an app. (rollback / rollforward)
#[derive(clap::Parser, Debug)]
pub struct CmdAppVersionActivate {
    #[clap(flatten)]
    pub env: WasmerEnv,

    #[clap(flatten)]
    pub fmt: ItemFormatOpts,

    /// App version ID to activate.
    ///
    /// This must be the unique version ID, not the version name!
    /// Eg: dav_xYzaB1aaaaax
    pub version: String,
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppVersionActivate {
    type Output = ();

    async fn run_async(self) -> Result<(), anyhow::Error> {
        let client = self.env.client()?;

        let app = wasmer_backend_api::query::app_version_activate(&client, self.version).await?;

        let Some(version) = &app.active_version else {
            anyhow::bail!("Failed to activate version: backend did not update version!");
        };

        eprintln!(
            "Changed active version of app '{}/{}' from '{}' to '{}' (id: {})",
            app.owner.global_name,
            app.name,
            version.version,
            version.version,
            version.id.inner(),
        );

        Ok(())
    }
}
