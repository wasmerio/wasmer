//! Manage app CDN cache.

use super::util::AppIdentOpts;
use crate::{commands::AsyncCliCommand, config::WasmerEnv};

pub mod disable;
pub mod enable;
pub mod purge;
pub mod status;

/// Manage CDN cache for an app.
#[derive(clap::Subcommand, Debug)]
pub enum CmdAppCdn {
    Enable(enable::CmdAppCdnEnable),
    Disable(disable::CmdAppCdnDisable),
    Status(status::CmdAppCdnStatus),
    Purge(purge::CmdAppCdnPurge),
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppCdn {
    type Output = ();

    async fn run_async(self) -> Result<Self::Output, anyhow::Error> {
        match self {
            Self::Enable(cmd) => cmd.run_async().await,
            Self::Disable(cmd) => cmd.run_async().await,
            Self::Status(cmd) => cmd.run_async().await,
            Self::Purge(cmd) => cmd.run_async().await,
        }
    }
}

async fn configure_cdn_cache(
    env: WasmerEnv,
    ident: AppIdentOpts,
    enabled: bool,
) -> Result<(), anyhow::Error> {
    let client = env.client()?;
    let (_ident, app) = ident.load_app(&client).await?;

    let payload = wasmer_backend_api::query::configure_app_cdn_cache(
        &client,
        wasmer_backend_api::types::ConfigureAppCdnCacheVars {
            app: app.id,
            enabled: Some(enabled),
        },
    )
    .await?;

    anyhow::ensure!(
        payload.success,
        "CDN cache configuration update was not successful"
    );

    println!(
        "CDN cache {}.",
        if enabled { "enabled" } else { "disabled" }
    );

    Ok(())
}
