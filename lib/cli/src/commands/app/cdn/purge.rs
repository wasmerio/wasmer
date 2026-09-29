use super::{AppIdentOpts, WasmerEnv};
use crate::commands::AsyncCliCommand;

/// Purge CDN cache for an app.
#[derive(clap::Parser, Debug)]
pub struct CmdAppCdnPurge {
    #[clap(flatten)]
    pub env: WasmerEnv,

    #[clap(flatten)]
    pub ident: AppIdentOpts,
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppCdnPurge {
    type Output = ();

    async fn run_async(self) -> Result<(), anyhow::Error> {
        let client = self.env.client()?;
        let (_ident, app) = self.ident.load_app(&client).await?;

        let payload = wasmer_backend_api::query::purge_app_cdn_cache(
            &client,
            wasmer_backend_api::types::PurgeAppCdnCacheVars { app: app.id },
        )
        .await?;

        anyhow::ensure!(payload.success, "CDN cache purge was not successful");
        println!("CDN cache purged.");

        Ok(())
    }
}
