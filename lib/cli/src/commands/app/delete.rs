//! Delete an Edge app.

use dialoguer::Confirm;
use is_terminal::IsTerminal;

use super::util::AppIdentOpts;
use crate::{commands::AsyncCliCommand, config::WasmerEnv};

/// Delete an existing Edge app
#[derive(clap::Parser, Debug)]
pub struct CmdAppDelete {
    #[clap(flatten)]
    env: WasmerEnv,

    #[clap(long, default_value_t = !std::io::stdin().is_terminal())]
    non_interactive: bool,

    #[clap(flatten)]
    ident: AppIdentOpts,
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppDelete {
    type Output = ();

    async fn run_async(self) -> Result<(), anyhow::Error> {
        let interactive = !self.non_interactive;
        let client = self.env.client()?;

        eprintln!("Looking up the app...");
        let (_ident, app) = self.ident.load_app(&client).await?;

        if interactive {
            let theme = dialoguer::theme::ColorfulTheme::default();
            let should_use = Confirm::with_theme(&theme)
                .with_prompt(format!(
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
        wasmer_backend_api::query::delete_app(&client, app.id.into_inner()).await?;

        eprintln!("App '{}/{}' was deleted!", app.owner.global_name, app.name);

        Ok(())
    }
}
