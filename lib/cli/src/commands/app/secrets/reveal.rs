use super::utils;
use crate::{
    commands::{app::util::AppIdentFlag, AsyncCliCommand},
    config::WasmerEnv,
    opts::ListFormatOpts,
    utils::render::{ItemFormat, ListFormat},
};
use dialoguer::theme::ColorfulTheme;
use is_terminal::IsTerminal;
use std::path::PathBuf;

/// Reveal the value of an existing app secret.
#[derive(clap::Parser, Debug)]
pub struct CmdAppSecretsReveal {
    /* --- Common flags --- */
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

    /* --- Flags --- */
    #[clap(flatten)]
    pub app_id: AppIdentFlag,

    /// The path to the directory where the config file for the application will be written to.
    #[clap(long = "app-dir", conflicts_with = "app")]
    pub app_dir_path: Option<PathBuf>,

    /// Reveal all the secrets related to an app.
    #[clap(long, conflicts_with = "name")]
    pub all: bool,

    /* --- Parameters --- */
    /// The name of the secret to get the value of.
    #[clap(name = "name")]
    pub secret_name: Option<String>,
}

impl CmdAppSecretsReveal {
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
        let client = self.env.client()?;
        let app_id = super::utils::get_app_id(
            &client,
            self.app_id.app.as_ref(),
            self.app_dir_path.as_ref(),
            self.quiet,
            self.non_interactive,
        )
        .await?;

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
