//! Show short information about an Edge app.

use crate::{
    commands::{app::get::CmdAppGet, AsyncCliCommand},
    opts::{ApiOpts, ItemFormatOpts},
};

use super::util::AppIdentOpts;

/// Show short information about an Edge app.
///
/// Use `app get` to get more detailed information.
#[derive(clap::Parser, Debug)]
pub struct CmdAppInfo {
    #[clap(flatten)]
    api: ApiOpts,
    #[clap(flatten)]
    fmt: ItemFormatOpts,

    #[clap(flatten)]
    ident: AppIdentOpts,
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppInfo {
    type Output = ();

    async fn run_async(self) -> Result<(), anyhow::Error> {
        let cmd_app_get = CmdAppGet {
            api: self.api,
            fmt: self.fmt,
            ident: self.ident,
        };
        let app = cmd_app_get.run_async().await?;

        let app_url = app.url;
        let versioned_url = app.active_version.url;
        let dashboard_url = app.admin_url;

        eprintln!("  App Info  ");
        eprintln!("> App Name: {}", app.name);
        eprintln!("> App URL: {}", app_url);
        eprintln!("> Versioned URL: {}", versioned_url);
        eprintln!("> Admin dashboard: {}", dashboard_url);

        Ok(())
    }
}
