use anyhow::Context;

use crate::{
    commands::{app::util::AppIdentOpts, AsyncCliCommand},
    config::WasmerEnv,
    opts::ItemFormatOpts,
    utils::render::ItemFormat,
};

/// Show information for a specific app version.
#[derive(clap::Parser, Debug)]
pub struct CmdAppVersionGet {
    #[clap(flatten)]
    pub env: WasmerEnv,

    #[clap(flatten)]
    pub fmt: ItemFormatOpts,

    /// *Name* of the version - NOT the unique version id!
    #[clap(long)]
    pub name: String,

    #[clap(flatten)]
    #[allow(missing_docs)]
    pub ident: AppIdentOpts,
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppVersionGet {
    type Output = ();

    async fn run_async(self) -> Result<(), anyhow::Error> {
        let client = self.env.client()?;
        let (_ident, app) = self.ident.load_app(&client).await?;

        let version = wasmer_backend_api::query::get_app_version(
            &client,
            app.owner.global_name,
            app.name,
            self.name.clone(),
        )
        .await?
        .with_context(|| format!("Could not find app version '{}'", self.name))?;

        println!(
            "{}",
            self.fmt.get_with_default(ItemFormat::Yaml).render(&version)
        );

        Ok(())
    }
}
