use super::AsyncCliCommand;
use crate::{
    commands::deploy::deploy::DeployApp,
    opts::{ApiOpts, ItemFormatOpts},
};
use anyhow::Context;
use edge_schema::schema::AppConfigV1;
use std::path::PathBuf;
use wasmer_api::types::DeployAppVersion;

// [todo]: deploy inside deploy? Let's think of a better name.
mod deploy;

/// Deploy an app to Wasmer Edge.
#[derive(clap::Parser, Debug)]
pub struct CmdDeploy {
    #[clap(flatten)]
    pub api: ApiOpts,

    #[clap(flatten)]
    pub fmt: ItemFormatOpts,

    /// Skip local schema validation.
    #[clap(long)]
    pub no_validate: bool,

    /// Do not prompt for user input.
    #[clap(long)]
    pub non_interactive: bool,

    /// Automatically publish the package referenced by this app.
    ///
    /// Only works if the corresponding wasmer.toml is in the same directory.
    #[clap(long)]
    pub publish_package: bool,

    /// The path to the app.yaml file.
    #[clap(long)]
    pub path: Option<PathBuf>,

    /// Do not wait for the app to become reachable.
    #[clap(long)]
    pub no_wait: bool,

    /// Do not make the new app version the default (active) version.
    /// This is useful for testing a deployment first, before moving it to "production".
    #[clap(long)]
    pub no_default: bool,

    /// Do not persist the app version ID in the app.yaml.
    #[clap(long)]
    pub no_persist_id: bool,

    /// Specify the owner (user or namespace) of the app.
    /// Will default to the currently logged in user, or the existing one
    /// if the app can be found.
    #[clap(long)]
    pub owner: Option<String>,
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdDeploy {
    type Output = DeployAppVersion;

    async fn run_async(self) -> Result<DeployAppVersion, anyhow::Error> {
        let app_path = {
            let base_path = self.path.clone().unwrap_or(std::env::current_dir()?);
            if base_path.is_file() {
                base_path
            } else if base_path.is_dir() {
                let f = base_path.join(AppConfigV1::CANONICAL_FILE_NAME);
                if !f.is_file() {
                    anyhow::bail!("Could not find app.yaml at path '{}'", f.display());
                }

                f
            } else {
                anyhow::bail!("No such file or directory '{}'", base_path.display());
            }
        };

        let config_str = std::fs::read_to_string(&app_path)
            .with_context(|| format!("Could not read file '{}'", app_path.display()))?;

        let config: AppConfigV1 = AppConfigV1::parse_yaml(&config_str)?;
        eprintln!("Loaded app from: '{}'", app_path.display());

        Into::<DeployApp>::into(config).deploy(app_path, &self).await
    }
}
