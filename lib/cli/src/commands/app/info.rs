//! Show short information about an Edge app.

use super::util::AppIdentOpts;
use crate::{commands::AsyncCliCommand, opts::ApiOpts};

/// Show short information about an Edge app.
///
/// Use `app get` to get more detailed information.
#[derive(clap::Parser, Debug)]
pub struct CmdAppInfo {
    #[clap(flatten)]
    api: ApiOpts,
    #[clap(flatten)]
    ident: AppIdentOpts,
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppInfo {
    type Output = ();

    async fn run_async(self) -> Result<(), anyhow::Error> {
        let client = self.api.client()?;
        let (_ident, app) = self.ident.load_app(&client).await?;

        let app_url = app.url;
        let versioned_url = app.active_version.url;
        let dashboard_url = app.admin_url;

        println!("  App Info  ");
        println!("> App Name: {}", app.name);
        println!("> Namespace: {}", app.owner.global_name);
        println!("> App URL: {}", app_url);
        println!("> Versioned URL: {}", versioned_url);
        println!("> Admin dashboard: {}", dashboard_url);

        Ok(())
    }
}
