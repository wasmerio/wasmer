use super::utils::Secret;
use crate::{
    commands::{app::util::AppIdentFlag, AsyncCliCommand},
    config::WasmerEnv,
};
use anyhow::Context;
use colored::Colorize;
use dialoguer::theme::ColorfulTheme;
use is_terminal::IsTerminal;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};
use wasmer_backend_api::WasmerClient;

/// Update an existing app secret.
#[derive(clap::Parser, Debug)]
pub struct CmdAppSecretsUpdate {
    /* --- Common args --- */
    #[clap(flatten)]
    pub env: WasmerEnv,

    /// Don't print any message.
    #[clap(long)]
    pub quiet: bool,

    /// Do not prompt for user input.
    #[clap(long, default_value_t = !std::io::stdin().is_terminal())]
    pub non_interactive: bool,

    /* --- Flags --- */
    /// The path to the directory where the config file for the application will be written to.
    #[clap(long = "app-dir", conflicts_with = "app")]
    pub app_dir_path: Option<PathBuf>,

    #[clap(flatten)]
    pub app_id: AppIdentFlag,

    /// Path to a file with secrets stored in JSON format to update secrets from.
    #[clap(
        long,
        name = "from-file",
        conflicts_with = "value",
        conflicts_with = "name"
    )]
    pub from_file: Option<PathBuf>,

    /// Whether or not to redeploy the app after creating the secrets.
    #[clap(long)]
    pub redeploy: bool,

    /* --- Parameters --- */
    /// The name of the secret to update.
    #[clap(name = "name")]
    pub secret_name: Option<String>,

    /// The value of the secret to update.
    #[clap(name = "value")]
    pub secret_value: Option<String>,
}

impl CmdAppSecretsUpdate {
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

    fn get_secret_value(&self) -> anyhow::Result<String> {
        if let Some(value) = &self.secret_value {
            return Ok(value.clone());
        }

        if self.non_interactive {
            anyhow::bail!("No secret value given. Provide one as a positional argument.")
        } else {
            let theme = ColorfulTheme::default();
            Ok(dialoguer::Input::with_theme(&theme)
                .with_prompt("Enter the value of the secret:")
                .interact_text()?)
        }
    }

    /// Given a list of secrets, checks if the given secrets already exist for the given app and
    /// returns a list of secrets that must be upserted.
    async fn filter_secrets(
        &self,
        client: &WasmerClient,
        app_id: &str,
        secrets: Vec<Secret>,
    ) -> anyhow::Result<Vec<Secret>> {
        let names = secrets.iter().map(|s| &s.name);
        let app_secrets =
            wasmer_backend_api::query::get_all_app_secrets_filtered(client, app_id, names).await?;
        let sset = HashSet::<&str>::from_iter(app_secrets.iter().map(|s| s.name.as_str()));

        let mut ret = vec![];

        for secret in secrets {
            if !sset.contains(secret.name.as_str()) {
                if self.non_interactive {
                    anyhow::bail!("Cannot update secret '{}' in app {app_id} as it does not exist yet. Use the `create` command instead.", secret.name.bold());
                } else {
                    eprintln!(
                        "Secret '{}' does not exist for the selected app.",
                        secret.name.bold()
                    );
                    let theme = ColorfulTheme::default();
                    let res = dialoguer::Confirm::with_theme(&theme)
                        .with_prompt("Do you want to create it?")
                        .interact()?;

                    if !res {
                        eprintln!("Cannot update secret '{}' as it does not exist yet. Use the `create` command instead.", secret.name.bold());
                    }
                }
            }

            ret.push(secret);
        }

        Ok(ret)
    }
    async fn update(
        &self,
        client: &WasmerClient,
        app_id: &str,
        secrets: Vec<Secret>,
    ) -> Result<(), anyhow::Error> {
        let res = wasmer_backend_api::query::upsert_app_secrets(
            client,
            app_id,
            secrets.iter().map(|s| (s.name.as_str(), s.value.as_str())),
        )
        .await?;
        let res = res.context(
            "Backend did not return any payload to confirm the successful update of the secret!",
        )?;

        if !res.success {
            anyhow::bail!("Secret creation failed!")
        } else {
            if !self.quiet {
                eprintln!("Succesfully updated secret(s):");
                for secret in &secrets {
                    eprintln!("{}", secret.name.bold());
                }

                let should_redeploy = self.redeploy || {
                    if !self.non_interactive && self.from_file.is_some() {
                        let theme = ColorfulTheme::default();
                        dialoguer::Confirm::with_theme(&theme)
                            .with_prompt("Do you want to redeploy your app?")
                            .interact()?
                    } else {
                        false
                    }
                };

                if should_redeploy {
                    wasmer_backend_api::query::redeploy_app_by_id(client, app_id).await?;
                    eprintln!("{} Deployment complete", "𖥔".yellow().bold());
                } else {
                    eprintln!(
                        "{}: In order for secrets to appear in your app, re-deploy it.",
                        "Info".bold()
                    );
                }
            }

            Ok(())
        }
    }

    async fn update_from_file(
        &self,
        client: &WasmerClient,
        path: &Path,
        app_id: &str,
    ) -> anyhow::Result<(), anyhow::Error> {
        let secrets = super::utils::read_secrets_from_file(path).await?;

        let secrets = self.filter_secrets(client, app_id, secrets).await?;
        self.update(client, app_id, secrets).await?;

        Ok(())
    }
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppSecretsUpdate {
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
            self.update_from_file(&client, file, &app_id).await
        } else {
            let name = self.get_secret_name()?;
            let value = self.get_secret_value()?;
            let secret = Secret { name, value };
            self.update(&client, &app_id, vec![secret]).await
        }
    }
}
