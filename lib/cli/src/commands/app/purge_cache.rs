//! Get information about an edge app.

use anyhow::Context;

use super::util::AppIdentOpts;
use crate::{commands::AsyncCliCommand, config::WasmerEnv, opts::ItemFormatOpts};

/// Purge caches for applications.
///
/// Cache scopes that can be cleared:
/// * InstaBoot startup snapshots
///   Will delete all existing snapshots.
///   New snapshots will be created automatically.
#[derive(clap::Parser, Debug)]
pub struct CmdAppPurgeCache {
    #[clap(flatten)]
    pub env: WasmerEnv,

    #[clap(flatten)]
    pub fmt: ItemFormatOpts,

    #[clap(flatten)]
    pub ident: AppIdentOpts,
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppPurgeCache {
    type Output = ();

    async fn run_async(self) -> Result<(), anyhow::Error> {
        let client = self.env.client()?;
        let (_ident, app) = self.ident.load_app(&client).await?;

        let version_id = app
            .active_version
            .as_ref()
            .context("Could not purge cache: app has no active version!")?
            .id
            .clone();

        let name = format!("{} ({})", app.name, app.owner.global_name);

        println!(
            "Purging caches for {}, app version {}...",
            name,
            version_id.inner()
        );

        let vars = wasmer_backend_api::types::PurgeCacheForAppVersionVars { id: version_id };
        wasmer_backend_api::query::purge_cache_for_app_version(&client, vars).await?;

        println!("🚽 swirl! All caches have been purged!");

        Ok(())
    }
}
