use crate::{
    commands::{app::util::AppIdentFlag, AsyncCliCommand},
    config::WasmerEnv,
};
use colored::Colorize;
use dialoguer::theme::ColorfulTheme;
use is_terminal::IsTerminal;
use std::path::{Path, PathBuf};
use wasmer_backend_api::WasmerClient;

use super::utils::{self, get_secrets};

/// Delete an existing app secret.
#[derive(clap::Parser, Debug)]
pub struct CmdAppSecretsDelete {
    /* --- Common flags --- */
    #[clap(flatten)]
    pub env: WasmerEnv,

    /// Don't print any message.
    #[clap(long)]
    pub quiet: bool,

    /// Do not prompt for user input.
    #[clap(long, default_value_t = !std::io::stdin().is_terminal())]
    pub non_interactive: bool,

    /* --- Flags --- */
    #[clap(flatten)]
    pub app_id: AppIdentFlag,

    /// The path to the directory where the config file for the application will be written to.
    #[clap(long = "app-dir", conflicts_with = "app")]
    pub app_dir_path: Option<PathBuf>,

    /// Path to a file with secrets stored in JSON format to delete secrets from.
    #[clap(
        long,
        name = "from-file",
        conflicts_with = "name",
        conflicts_with = "all"
    )]
    pub from_file: Option<PathBuf>,

    /// Delete all the secrets related to an app.
    #[clap(long, conflicts_with = "name")]
    pub all: bool,

    /// Delete the secret(s) without asking for confirmation.
    #[clap(long)]
    pub force: bool,

    /* --- Parameters --- */
    /// The name of the secret to delete.
    #[clap(name = "name")]
    pub secret_name: Option<String>,
}

impl CmdAppSecretsDelete {
    fn get_secret_name(&self) -> anyhow::Result<String> {
        if let Some(name) = &self.secret_name {
            return Ok(name.clone());
        }

        if self.non_interactive {
            anyhow::bail!("No secret name given. Provide one as a positional argument.")
        } else {
            let theme = ColorfulTheme::default();
            Ok(dialoguer::Input::with_theme(&theme)
                .with_prompt("Enter the name of the secret")
                .interact_text()?)
        }
    }

    async fn delete(
        &self,
        client: &WasmerClient,
        app_id: &str,
        secret_name: &str,
    ) -> anyhow::Result<()> {
        let secret = utils::get_secret_by_name(client, app_id, secret_name).await?;

        if let Some(secret) = secret {
            if !self.non_interactive && !self.force {
                let theme = ColorfulTheme::default();
                let res = dialoguer::Confirm::with_theme(&theme)
                    .with_prompt(format!("Delete secret '{}'?", secret_name.bold()))
                    .interact()?;
                if !res {
                    return Ok(());
                }
            }

            let res = wasmer_backend_api::query::delete_app_secret(client, secret.id.into_inner())
                .await?;

            match res {
                Some(res) if !res.success => {
                    anyhow::bail!("Error deleting secret '{}'", secret.name.bold())
                }
                Some(_) => {
                    if !self.quiet {
                        eprintln!("Correctly deleted secret '{}'", secret.name.bold());
                    }
                    Ok(())
                }
                None => {
                    anyhow::bail!("Error deleting secret '{}'", secret.name.bold())
                }
            }
        } else {
            if !self.quiet {
                eprintln!("No secret with name '{}' found", secret_name.bold());
            }
            Ok(())
        }
    }

    async fn delete_from_file(
        &self,
        client: &WasmerClient,
        path: &Path,
        app_id: String,
    ) -> anyhow::Result<()> {
        let secrets = super::utils::read_secrets_from_file(path).await?;

        for secret in secrets {
            self.delete(client, &app_id, &secret.name).await?;
        }

        Ok(())
    }
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppSecretsDelete {
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
        if let Some(file) = &self.from_file {
            self.delete_from_file(&client, file, app_id).await
        } else if self.all {
            if self.non_interactive && !self.force {
                anyhow::bail!("Refusing to delete all secrets in non-interactive mode without the `--force` flag.")
            }
            let secrets = get_secrets(&client, &app_id).await?;
            for secret in secrets {
                self.delete(&client, &app_id, &secret.name).await?;
            }

            Ok(())
        } else {
            let name = self.get_secret_name()?;
            self.delete(&client, &app_id, &name).await
        }
    }
}
