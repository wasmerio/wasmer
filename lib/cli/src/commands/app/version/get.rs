use anyhow::Context;

use crate::{
    commands::{app::util::AppIdentOpts, AsyncCliCommand},
    opts::{ApiOpts, ItemFormatOpts},
};

/// Show information for a specific app version.
#[derive(clap::Parser, Debug)]
pub struct CmdAppVersionGet {
    #[clap(flatten)]
    #[allow(missing_docs)]
    pub api: ApiOpts,
    #[clap(flatten)]
    #[allow(missing_docs)]
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
        let client = self.api.client()?;
        let (_ident, app) = self.ident.load_app(&client).await?;

        let version = wasmer_api::query::get_app_version(
            &client,
            app.owner.global_name,
            app.name,
            self.name.clone(),
        )
        .await?
        .with_context(|| format!("Could not find app version '{}'", self.name))?;

        println!("{}", self.fmt.format.render(&version));

        Ok(())
    }
}
