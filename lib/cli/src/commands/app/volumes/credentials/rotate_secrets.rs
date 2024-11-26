use super::ItemFormatOpts;
use crate::{
    commands::{
        app::util::{AppIdent, AppIdentOpts},
        AsyncCliCommand,
    },
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
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppVolumesRotateSecrets {
    type Output = ();

    async fn run_async(self) -> Result<Self::Output, anyhow::Error> {
        let client = self.env.client()?;
        let (_ident, app) = self.ident.load_app(&client).await?;

        wasmer_backend_api::query::rotate_s3_secrets(&client, app.id).await?;

        // Don't print it here and leave it implied, so users can append the output
        // of `wasmer app volumes credentials` without worrying about this message.
        //
        //println!(
        //    "Correctly rotated s3 secrets for app {} ({})",
        //    app.name,
        //    app.owner.global_name.bold()
        //);

        super::CmdAppVolumesCredentials {
            env: self.env,
            fmt: self.fmt,
            ident: AppIdentOpts {
                app: Some(AppIdent::NamespacedName(app.owner.global_name, app.name)),
            },
        }
        .run_async()
        .await?;

        Ok(())
    }
}
