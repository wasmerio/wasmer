use super::utils;
use crate::{
    commands::{AsyncCliCommand, app::util::AppIdentFlag},
    config::WasmerEnv,
};
use std::io::IsTerminal as _;
use std::path::PathBuf;

#[derive(clap::Parser, Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum SecretExportFormat {
    #[default]
    #[clap(name = "env")]
    Env,
    #[clap(name = "json")]
    Json,
    #[clap(name = "yaml")]
    Yaml,
}

/// Export app secrets to a file or stdout.
#[derive(clap::Parser, Debug)]
pub struct CmdAppSecretsExport {
    #[clap(flatten)]
    pub env: WasmerEnv,

    #[clap(long)]
    pub quiet: bool,

    #[clap(long, default_value_t = !std::io::stdin().is_terminal())]
    pub non_interactive: bool,

    #[clap(flatten)]
    pub app_id: AppIdentFlag,

    #[clap(long = "app-dir", conflicts_with = "app")]
    pub app_dir_path: Option<PathBuf>,

    #[clap(short = 'f', long, default_value = "env")]
    pub format: SecretExportFormat,

    #[clap(short = 'o', long)]
    pub output: Option<PathBuf>,
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppSecretsExport {
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

        if !self.quiet {
            eprintln!("Exporting secrets for app {app_id}...");
        }

        let secrets: Vec<utils::Secret> = utils::reveal_secrets(&client, &app_id).await?;

        let output = match self.format {
            SecretExportFormat::Env => {
                let mut lines = Vec::new();
                for secret in &secrets {
                    lines.push(format!(
                        "{}=\"{}\"",
                        secret.name,
                        utils::render::sanitize_value(&secret.value)
                    ));
                }
                lines.join("\n")
            }
            SecretExportFormat::Json => serde_json::to_string_pretty(&secrets)?,
            SecretExportFormat::Yaml => serde_yaml::to_string(&secrets)?,
        };

        if let Some(path) = &self.output {
            std::fs::write(path, output)?;
            if !self.quiet {
                eprintln!("Secrets exported to {}", path.display());
            }
        } else {
            println!("{output}");
        }

        Ok(())
    }
}
