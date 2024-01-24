use crate::{cmd::AsyncCliCommand, ApiOpts, ItemFormatOpts};

/// Switch the active version of an app. (rollback / rollforward)
#[derive(clap::Parser, Debug)]
pub struct CmdAppVersionActivate {
    #[clap(flatten)]
    pub api: ApiOpts,
    #[clap(flatten)]
    pub fmt: ItemFormatOpts,

    /// *ID* of the version, NOT the version name!
    ///
    /// Eg: dav_xYzaB1aaaaax
    pub version: String,
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppVersionActivate {
    type Output = ();

    async fn run_async(self) -> Result<(), anyhow::Error> {
        let client = self.api.client()?;

        let app = wasmer_api::query::app_version_activate(&client, self.version).await?;

        eprintln!(
            "Changed active version of app '{}/{}' from '{}' to '{}' (id: {})",
            app.owner.global_name,
            app.name,
            app.active_version.version,
            app.active_version.version,
            app.active_version.id.inner()
        );

        Ok(())
    }
}
