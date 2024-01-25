//! Delete an Edge app.

use dialoguer::Confirm;
use is_terminal::IsTerminal;

use super::util::AppIdentOpts;
use crate::{commands::AsyncCliCommand, opts::ApiOpts};

/// Show an app.
#[derive(clap::Parser, Debug)]
pub struct CmdAppDelete {
    #[clap(flatten)]
    api: ApiOpts,

    #[clap(long)]
    non_interactive: bool,

    #[clap(flatten)]
    ident: AppIdentOpts,
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppDelete {
    type Output = ();

    async fn run_async(self) -> Result<(), anyhow::Error> {
        let interactive = std::io::stdin().is_terminal() && !self.non_interactive;
        let client = self.api.client()?;

        eprintln!("Looking up the app...");
        let (_ident, app) = self.ident.load_app(&client).await?;

        if interactive {
            let should_use = Confirm::new()
                .with_prompt(&format!(
                    "Really delete the app '{}/{}'? (id: {})",
                    app.owner.global_name,
                    app.name,
                    app.id.inner()
                ))
                .interact()?;

            if !should_use {
                eprintln!("App will not be deleted.");
                return Ok(());
            }
        }

        eprintln!(
            "Deleting app {}/{} (id: {})...",
            app.owner.global_name,
            app.name,
            app.id.inner(),
        );
        wasmer_api::query::delete_app(&client, app.id.into_inner()).await?;

        eprintln!("App '{}/{}' was deleted!", app.owner.global_name, app.name);

        Ok(())
    }
}
