use super::utils::{get_secrets, BackendSecretWrapper};
use crate::{
    commands::{
        app::util::{get_app_config_from_dir, prompt_app_ident, AppIdent},
        AsyncCliCommand,
    },
    opts::{ApiOpts, ListFormatOpts, WasmerEnv},
};
use colored::Colorize;
use is_terminal::IsTerminal;
use std::{env::current_dir, path::PathBuf, str::FromStr};
use wasmer_api::WasmerClient;

/// Retrieve the value of an existing secret related to an Edge app.
#[derive(clap::Parser, Debug)]
pub struct CmdAppSecretsList {
    /// The identifier of the app to list secrets of.
    #[clap(name = "app-id")]
    pub app_id: Option<AppIdent>,

    /// The path to the directory where the config file for the application will be written to.
    #[clap(long = "app-dir", conflicts_with = "app-id")]
    pub app_dir_path: Option<PathBuf>,

    /* --- Common args --- */
    #[clap(flatten)]
    #[allow(missing_docs)]
    pub api: ApiOpts,

    #[clap(flatten)]
    pub env: WasmerEnv,

    /// Don't print any message.
    #[clap(long)]
    pub quiet: bool,

    /// Do not prompt for user input.
    #[clap(long, default_value_t = !std::io::stdin().is_terminal())]
    pub non_interactive: bool,

    #[clap(flatten)]
    pub fmt: ListFormatOpts,
}

impl CmdAppSecretsList {
    async fn get_app_id(&self, client: &WasmerClient) -> anyhow::Result<String> {
        if let Some(app_id) = &self.app_id {
            let app = app_id.resolve(client).await?;
            return Ok(app.id.into_inner());
        }

        let app_dir_path = if let Some(app_dir_path) = &self.app_dir_path {
            app_dir_path.clone()
        } else {
            current_dir()?
        };

        if let Ok(r) = get_app_config_from_dir(&app_dir_path) {
            let (app, _) = r;

            let id = if let Some(id) = &app.app_id {
                Some(id.clone())
            } else if let Ok(app_ident) = AppIdent::from_str(&app.name) {
                let app = app_ident.resolve(client).await?;
                Some(app.id.into_inner())
            } else {
                None
            };

            if let Some(id) = id {
                if !self.quiet {
                    if let Some(owner) = &app.owner {
                        eprintln!(
                            "Managing secrets related to app {} ({owner}).",
                            app.name.bold()
                        );
                    } else {
                        eprintln!("Managing secrets related to app {}.", app.name.bold());
                    }
                }
                return Ok(id);
            }
        } else if let Some(path) = &self.app_dir_path {
            anyhow::bail!(
                "No app configuration file found in path {}.",
                path.display()
            )
        }

        if self.non_interactive {
            anyhow::bail!("No app id given. Provide one as a positional argument.")
        } else {
            let id = prompt_app_ident("Enter the name of the app")?;
            let app = id.resolve(client).await?;
            Ok(app.id.into_inner())
        }
    }
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppSecretsList {
    type Output = ();

    async fn run_async(self) -> Result<Self::Output, anyhow::Error> {
        let client = self.api.client()?;
        let app_id = self.get_app_id(&client).await?;
        let secrets: Vec<BackendSecretWrapper> = get_secrets(&client, &app_id)
            .await?
            .into_iter()
            .map(|s| s.into())
            .collect();

        println!("{}", self.fmt.format.render(secrets.as_slice()));

        Ok(())
    }
}
