use crate::{
    cmd::{app::get::CmdAppGet, AsyncCliCommand},
    ApiOpts, ItemFormatOpts,
};

use super::util::AppIdentOpts;

#[derive(clap::Parser, Debug)]
pub struct CmdAppInfo {
    #[clap(flatten)]
    api: ApiOpts,
    #[clap(flatten)]
    fmt: ItemFormatOpts,

    #[clap(flatten)]
    ident: AppIdentOpts,
}

impl CmdAppInfo {
    async fn run(self) -> Result<(), anyhow::Error> {
        let cmd_app_get = CmdAppGet {
            api: self.api,
            fmt: self.fmt,
            ident: self.ident,
        };
        let app = cmd_app_get.get_app().await?;

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

impl AsyncCliCommand for CmdAppInfo {
    fn run_async(self) -> futures::future::BoxFuture<'static, Result<(), anyhow::Error>> {
        Box::pin(self.run())
    }
}
