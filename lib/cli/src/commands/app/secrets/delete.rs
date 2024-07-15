use crate::{
    commands::{app::util::{get_app_id_from_config, AppIdent, prompt_app_ident}, AsyncCliCommand},
    opts::{ApiOpts, WasmerEnv},
};
use colored::Colorize;
use dialoguer::theme::ColorfulTheme;
use is_terminal::IsTerminal;
use std::{
    env::current_dir,
    path::{Path, PathBuf},
};
use wasmer_api::WasmerClient;

use super::utils::{self, get_secrets};

/// Delete an existing secret related to an Edge app.
#[derive(clap::Parser, Debug)]
pub struct CmdAppSecretsDelete {
    /// The id of the app the secret is related to.
    pub app_id: Option<AppIdent>,

    /// The path to the directory where the config file for the application will be written to.
    #[clap(long = "app-dir", conflicts_with = "app-id")]
    pub app_dir_path: Option<PathBuf>,

    /// The name of the secret to delete.
    #[clap(name = "name")]
    pub secret_name: Option<String>,

    /// Path to a file with secrets stored in JSON format to delete secrets from.
    #[clap(
        long,
        name = "from-file",
        conflicts_with = "secret_value",
        conflicts_with = "secret_name"
    )]
    pub from_file: Option<PathBuf>,

    /// Delete all the secrets related to an app.
    #[clap(long, conflicts_with = "name")]
    pub all: bool,

    /// Delete the secret(s) without asking for confirmation.
    #[clap(long)]
    pub force: bool,

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
}

impl CmdAppSecretsDelete {
    fn get_secret_name(&self) -> anyhow::Result<String> {
        if let Some(name) = &self.secret_name {
            return Ok(name.clone());
        }

        if self.non_interactive {
            anyhow::bail!("No secret name given. Use the `--name` flag to specify one.")
        } else {
            let theme = ColorfulTheme::default();
            Ok(dialoguer::Input::with_theme(&theme)
                .with_prompt("Enter the name of the secret")
                .interact_text()?)
        }
    }

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

        if let Ok(Some(app_id)) = get_app_id_from_config(&app_dir_path).await {
            return Ok(app_id.clone());
        }

        if self.non_interactive {
            anyhow::bail!("No app id given. Use the `--app_id` flag to specify one.")
        } else {
            let id = prompt_app_ident("Enter the name of the app")?;
            let app = id.resolve(client).await?;
            return Ok(app.id.into_inner());
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

            let res = wasmer_api::query::delete_app_secret(client, secret.id.into_inner()).await?;

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

    async fn delete_from_file(&self, path: &Path, app_id: String) -> anyhow::Result<()> {
        let secrets = super::utils::read_secrets_from_file(path).await?;
        let client = self.api.client()?;

        for secret in secrets {
            self.delete(&client, &app_id, &secret.name).await?;
        }

        Ok(())
    }
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppSecretsDelete {
    type Output = ();

    async fn run_async(self) -> Result<Self::Output, anyhow::Error> {
        let client = self.api.client()?;
        let app_id = self.get_app_id(&client).await?;

        if let Some(file) = &self.from_file {
            self.delete_from_file(file, app_id).await
        } else if self.all {
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
