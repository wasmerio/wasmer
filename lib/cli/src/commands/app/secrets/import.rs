use super::utils::Secret;
use crate::{
    commands::{AsyncCliCommand, app::util::AppIdentFlag},
    config::WasmerEnv,
};
use anyhow::Context;
use colored::Colorize;
use dialoguer::theme::ColorfulTheme;
use std::io::IsTerminal as _;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};
use wasmer_backend_api::WasmerClient;

#[derive(clap::Parser, Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum SecretImportFormat {
    #[default]
    #[clap(name = "env")]
    Env,
    #[clap(name = "json")]
    Json,
    #[clap(name = "yaml")]
    Yaml,
}

/// Import app secrets from a file.
#[derive(clap::Parser, Debug)]
pub struct CmdAppSecretsImport {
    #[clap(flatten)]
    pub env: WasmerEnv,

    #[clap(long)]
    pub quiet: bool,

    #[clap(long, default_value_t = !std::io::stdin().is_terminal())]
    pub non_interactive: bool,

    #[clap(long = "app-dir", conflicts_with = "app")]
    pub app_dir_path: Option<PathBuf>,

    #[clap(flatten)]
    pub app_id: AppIdentFlag,

    #[clap(long, name = "from-file", required = true)]
    pub from_file: PathBuf,

    #[clap(short = 'f', long, default_value = "env")]
    pub format: SecretImportFormat,

    #[clap(long)]
    pub update_existing: bool,

    #[clap(long)]
    pub redeploy: bool,
}

impl CmdAppSecretsImport {
    fn read_secrets(&self, path: &Path) -> anyhow::Result<Vec<Secret>> {
        match self.format {
            SecretImportFormat::Env => {
                let mut ret = vec![];
                for item in dotenvy::from_path_iter(path)? {
                    let (name, value) = item?;
                    ret.push(Secret { name, value })
                }
                Ok(ret)
            }
            SecretImportFormat::Json => {
                let content = std::fs::read_to_string(path)?;
                let secrets: Vec<Secret> = serde_json::from_str(&content)?;
                Ok(secrets)
            }
            SecretImportFormat::Yaml => {
                let content = std::fs::read_to_string(path)?;
                let secrets: Vec<Secret> = serde_yaml::from_str(&content)?;
                Ok(secrets)
            }
        }
    }

    async fn filter_secrets(
        &self,
        client: &WasmerClient,
        app_id: &str,
        secrets: Vec<Secret>,
    ) -> anyhow::Result<Vec<Secret>> {
        let names = secrets.iter().map(|s| &s.name);
        let app_secrets =
            wasmer_backend_api::query::get_all_app_secrets_filtered(client, app_id, names).await?;
        let existing = HashSet::<String>::from_iter(app_secrets.iter().map(|s| s.name.clone()));
        let mut ret = HashMap::new();

        for secret in secrets {
            if existing.contains(&secret.name) {
                if self.update_existing {
                    ret.insert(secret.name, secret.value);
                } else if self.non_interactive {
                    anyhow::bail!(
                        "Secret '{}' already exists. Use --update-existing to update it.",
                        secret.name.bold()
                    );
                } else {
                    eprintln!(
                        "Secret '{}' already exists for the selected app.",
                        secret.name.bold()
                    );
                    let theme = ColorfulTheme::default();
                    let res = dialoguer::Confirm::with_theme(&theme)
                        .with_prompt("Do you want to update it?")
                        .interact()?;

                    if res {
                        ret.insert(secret.name, secret.value);
                    }
                }
            } else {
                ret.insert(secret.name, secret.value);
            }
        }

        Ok(ret
            .into_iter()
            .map(|(name, value)| Secret { name, value })
            .collect())
    }

    async fn import(
        &self,
        client: &WasmerClient,
        app_id: &str,
        secrets: Vec<Secret>,
    ) -> Result<(), anyhow::Error> {
        if secrets.is_empty() {
            if !self.quiet {
                eprintln!("No secrets to import.");
            }
            return Ok(());
        }

        let res = wasmer_backend_api::query::upsert_app_secrets(
            client,
            app_id,
            secrets.iter().map(|s| (s.name.as_str(), s.value.as_str())),
        )
        .await?;
        let res = res.context(
            "Backend did not return any payload to confirm the successful import of secrets!",
        )?;

        if !res.success {
            anyhow::bail!("Secret import failed!")
        } else {
            if !self.quiet {
                let action = if self.update_existing { "imported/updated" } else { "imported" };
                eprintln!("Successfully {action} secret(s):");
                for secret in &secrets {
                    eprintln!("{}", secret.name.bold());
                }

                let should_redeploy = self.redeploy || {
                    if !self.non_interactive {
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
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppSecretsImport {
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

        let secrets = self.read_secrets(&self.from_file)?;
        let secrets = self.filter_secrets(&client, &app_id, secrets).await?;
        self.import(&client, &app_id, secrets).await
    }
}
