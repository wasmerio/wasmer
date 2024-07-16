use super::utils;
use crate::{
    commands::{
        app::util::{get_app_config_from_dir, prompt_app_ident, AppIdent},
        AsyncCliCommand,
    },
    opts::{ApiOpts, ListFormatOpts, WasmerEnv},
    utils::render::{ItemFormat, ListFormat},
};
use colored::Colorize;
use dialoguer::theme::ColorfulTheme;
use is_terminal::IsTerminal;
use std::{env::current_dir, path::PathBuf, str::FromStr};
use wasmer_api::WasmerClient;

/// Reveal the value of an existing secret related to an Edge app.
#[derive(clap::Parser, Debug)]
pub struct CmdAppSecretsReveal {
    /// The name of the secret to get the value of.
    #[clap(name = "name")]
    pub secret_name: Option<String>,

    /// The id of the app the secret is related to.
    #[clap(name = "app-id")]
    pub app_id: Option<AppIdent>,

    /// The path to the directory where the config file for the application will be written to.
    #[clap(long = "app-dir", conflicts_with = "app-id")]
    pub app_dir_path: Option<PathBuf>,

    /// Reveal all the secrets related to an app.
    #[clap(long, conflicts_with = "name")]
    pub all: bool,

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
    pub fmt: Option<ListFormatOpts>,
}

impl CmdAppSecretsReveal {
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

    fn get_secret_name(&self) -> anyhow::Result<String> {
        if let Some(name) = &self.secret_name {
            return Ok(name.clone());
        }

        if self.non_interactive {
            anyhow::bail!("No secret name given. Provide one as a positional argument.")
        } else {
            let theme = ColorfulTheme::default();
            Ok(dialoguer::Input::with_theme(&theme)
                .with_prompt("Enter the name of the secret:")
                .interact_text()?)
        }
    }
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppSecretsReveal {
    type Output = ();

    async fn run_async(self) -> Result<Self::Output, anyhow::Error> {
        let client = self.api.client()?;
        let app_id = self.get_app_id(&client).await?;

        if !self.all {
            let name = self.get_secret_name()?;

            let value = utils::get_secret_value_by_name(&client, &app_id, &name).await?;

            let secret = utils::Secret { name, value };

            if let Some(fmt) = &self.fmt {
                let fmt = match fmt.format {
                    ListFormat::Json => ItemFormat::Json,
                    ListFormat::Yaml => ItemFormat::Yaml,
                    ListFormat::Table => ItemFormat::Table,
                    ListFormat::ItemTable => {
                        anyhow::bail!("The 'item-table' format is not available for single values.")
                    }
                };
                println!("{}", fmt.render(&secret));
            } else {
                print!("{}", secret.value);
            }
        } else {
            let secrets: Vec<utils::Secret> = utils::reveal_secrets(&client, &app_id).await?;

            if let Some(fmt) = &self.fmt {
                println!("{}", fmt.format.render(secrets.as_slice()));
            } else {
                for secret in secrets {
                    println!(
                        "{}=\"{}\"",
                        secret.name,
                        utils::render::sanitize_value(&secret.value)
                    );
                }
            }
        }

        Ok(())
    }
}
