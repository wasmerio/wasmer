use crate::{
    commands::{app::util::AppIdentOpts, AsyncCliCommand},
    config::WasmerEnv,
};

use colored::Colorize;

/// Rotate the secrets linked to volumes of an app.
#[derive(clap::Parser, Debug)]
pub struct CmdAppVolumesRotateSecrets {
    #[clap(flatten)]
    pub env: WasmerEnv,

    #[clap(flatten)]
    pub ident: AppIdentOpts,
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppVolumesRotateSecrets {
    type Output = ();

    async fn run_async(self) -> Result<Self::Output, anyhow::Error> {
        let client = self.env.client()?;
        let (_ident, app) = self.ident.load_app(&client).await?;

        wasmer_api::query::rotate_s3_secrets(&client, app.id).await?;

        println!(
            "Correctly rotated s3 secrets for app {} ({})",
            app.name,
            app.owner.global_name.bold()
        );

        Ok(())
    }
}
