//! Show short information about an Edge app.

use super::util::AppIdentOpts;
use crate::{commands::AsyncCliCommand, config::WasmerEnv};

/// Show short information about an Edge app.
///
/// Use `app get` to get more detailed information.
#[derive(clap::Parser, Debug)]
pub struct CmdAppInfo {
    #[clap(flatten)]
    env: WasmerEnv,
    #[clap(flatten)]
    ident: AppIdentOpts,
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppInfo {
    type Output = ();

    async fn run_async(self) -> Result<(), anyhow::Error> {
        let client = self.env.client()?;
        let (_ident, app) = self.ident.load_app(&client).await?;

        let app_url = app.url;
        let versioned_url = app
            .active_version
            .as_ref()
            .map_or("n/a", |version| &version.url);
        let dashboard_url = app.admin_url;

        println!("  App Info  ");
        println!("→ Name: {}", app.name);
        println!("→ Owner: {}", app.owner.global_name);
        println!("→ URL: {}", app_url);
        println!("→ Unique URL: {}", versioned_url);
        println!("→ Dashboard: {}", dashboard_url);

        Ok(())
    }
}
