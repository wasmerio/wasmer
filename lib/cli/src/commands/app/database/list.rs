//! List volumes tied to an edge app.

use super::super::util::AppIdentOpts;
use crate::{commands::AsyncCliCommand, config::WasmerEnv, opts::ListFormatOpts};

/// List the volumes of an app.
#[derive(clap::Parser, Debug)]
pub struct CmdAppDatabaseList {
    #[clap(flatten)]
    fmt: ListFormatOpts,

    #[clap(flatten)]
    env: WasmerEnv,

    #[clap(flatten)]
    ident: AppIdentOpts,

    #[clap(long, help = "Include password in the output")]
    pub with_password: bool,
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppDatabaseList {
    type Output = ();

    async fn run_async(self) -> Result<(), anyhow::Error> {
        let client = self.env.client()?;

        let (_ident, app) = self.ident.load_app(&client).await?;
        let mut dbs = wasmer_backend_api::query::get_app_databases(
            &client,
            &app.owner.global_name,
            &app.name,
        )
        .await?;

        if !self.with_password {
            dbs.iter_mut().for_each(|db| {
                db.password = None;
            });
        }

        if dbs.is_empty() {
            eprintln!("App {} has no databases!", app.name);
        } else {
            println!("{}", self.fmt.format.render(dbs.as_slice()));
        }

        Ok(())
    }
}
