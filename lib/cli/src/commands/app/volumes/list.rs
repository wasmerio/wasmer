//! List volumes tied to an edge app.

use super::super::util::AppIdentOpts;
use crate::{commands::AsyncCliCommand, config::WasmerEnv, opts::ListFormatOpts};

/// List the volumes of an app.
#[derive(clap::Parser, Debug)]
pub struct CmdAppVolumesList {
    #[clap(flatten)]
    fmt: ListFormatOpts,

    #[clap(flatten)]
    env: WasmerEnv,

    #[clap(flatten)]
    ident: AppIdentOpts,
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppVolumesList {
    type Output = ();

    async fn run_async(self) -> Result<(), anyhow::Error> {
        let client = self.env.client()?;

        let (_ident, app) = self.ident.load_app(&client).await?;
        let volumes =
            wasmer_backend_api::query::get_app_volumes(&client, &app.owner.global_name, &app.name)
                .await?;

        if volumes.is_empty() {
            eprintln!("App {} has no volumes!", app.name);
        } else {
            println!("{}", self.fmt.format.render(volumes.as_slice()));
        }

        Ok(())
    }
}
