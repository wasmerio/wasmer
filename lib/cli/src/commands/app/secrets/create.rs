use super::utils::Secret;
use crate::{
    commands::{
        app::util::{get_app_id_from_config, prompt_app_ident, AppIdent},
        AsyncCliCommand,
    },
    opts::{ApiOpts, WasmerEnv},
};
use anyhow::Context;
use colored::Colorize;
use dialoguer::theme::ColorfulTheme;
use is_terminal::IsTerminal;
use std::{
    collections::{HashMap, HashSet},
    env::current_dir,
    path::{Path, PathBuf},
};
use wasmer_api::WasmerClient;

/// Create a new secret related to an Edge app.
#[derive(clap::Parser, Debug)]
pub struct CmdAppSecretsCreate {
    /// The identifier of the app the secret is related to.
    pub app_id: Option<AppIdent>,

    /// The path to the directory where the config file for the application will be written to.
    #[clap(long = "app-dir", conflicts_with = "app-id")]
    pub app_dir_path: Option<PathBuf>,

    /// The name of the secret to create.
    #[clap(name = "name")]
    pub secret_name: Option<String>,

    /// The value of the secret to create.
    #[clap(name = "value")]
    pub secret_value: Option<String>,

    /// Path to a file with secrets stored in JSON format to create secrets from.
    #[clap(
        long,
        name = "from-file",
        conflicts_with = "secret_name",
        conflicts_with = "all"
    )]
    pub from_file: Option<PathBuf>,

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

impl CmdAppSecretsCreate {
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

    fn get_secret_value(&self) -> anyhow::Result<String> {
        if let Some(value) = &self.secret_value {
            return Ok(value.clone());
        }

        if self.non_interactive {
            anyhow::bail!("No secret value given. Use the `--value` flag to specify one.")
        } else {
            let theme = ColorfulTheme::default();
            Ok(dialoguer::Input::with_theme(&theme)
                .with_prompt("Enter the value of the secret")
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
            wasmer_api::query::get_all_app_secrets_filtered(client, app_id, names).await?;
        let mut sset = HashSet::<String>::from_iter(app_secrets.iter().map(|s| s.name.clone()));
        let mut ret = HashMap::new();

        for secret in secrets {
            if sset.contains(&secret.name) {
                if self.non_interactive {
                    anyhow::bail!("Cannot create secret '{}' in app {app_id} as it already exists. Use the `update` command instead.", secret.name.bold());
                } else {
                    eprintln!(
                        "Secret '{}' already exists for the selected app.",
                        secret.name.bold()
                    );
                    let theme = ColorfulTheme::default();
                    let res = dialoguer::Confirm::with_theme(&theme)
                        .with_prompt("Do you want to update it?")
                        .interact()?;

                    if !res {
                        eprintln!("Cannot create secret '{}' in app {app_id} as it already exists. Use the `update` command instead.", secret.name.bold());
                    }
                }
            }

            sset.insert(secret.name.clone());
            ret.insert(secret.name, secret.value);
        }

        Ok(ret
            .into_iter()
            .map(|(name, value)| Secret { name, value })
            .collect())
    }

    async fn create(
        &self,
        client: &WasmerClient,
        app_id: &str,
        secrets: Vec<Secret>,
    ) -> Result<(), anyhow::Error> {
        let res = wasmer_api::query::upsert_app_secrets(
            client,
            app_id,
            secrets.iter().map(|s| (s.name.as_str(), s.value.as_str())),
        )
        .await?;
        let res = res.context(
            "Backend did not return any payload to confirm the successful creation of the secret!",
        )?;

        if !res.success {
            anyhow::bail!("Secret creation failed!")
        } else {
            if !self.quiet {
                eprintln!("Succesfully created secret(s):");
                for secret in secrets {
                    eprintln!("{}", secret.name.bold());
                }
            }

            Ok(())
        }
    }

    async fn create_from_file(
        &self,
        path: &Path,
        app_id: &str,
    ) -> anyhow::Result<(), anyhow::Error> {
        let secrets = super::utils::read_secrets_from_file(path).await?;
        let client = self.api.client()?;

        let secrets = self.filter_secrets(&client, app_id, secrets).await?;
        self.create(&client, app_id, secrets).await?;

        Ok(())
    }
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppSecretsCreate {
    type Output = ();

    async fn run_async(self) -> Result<Self::Output, anyhow::Error> {
        let client = self.api.client()?;
        let app_id = self.get_app_id(&client).await?;
        if let Some(file) = &self.from_file {
            self.create_from_file(file, &app_id).await
        } else {
            let name = self.get_secret_name()?;
            let value = self.get_secret_value()?;
            let secret = Secret { name, value };
            self.create(&client, &app_id, vec![secret]).await
        }
    }
}
