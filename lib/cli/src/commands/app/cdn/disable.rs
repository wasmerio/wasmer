use super::{AppIdentOpts, WasmerEnv, configure_cdn_cache};
use crate::commands::AsyncCliCommand;

/// Disable CDN cache for an app.
#[derive(clap::Parser, Debug)]
pub struct CmdAppCdnDisable {
    #[clap(flatten)]
    pub env: WasmerEnv,

    #[clap(flatten)]
    pub ident: AppIdentOpts,
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppCdnDisable {
    type Output = ();

    async fn run_async(self) -> Result<(), anyhow::Error> {
        configure_cdn_cache(self.env, self.ident, false).await
    }
}
