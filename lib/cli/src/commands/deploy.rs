use std::{io::Write, path::PathBuf};

use anyhow::{bail, Context};
use edge_schema::schema::AppConfigV1;
use is_terminal::IsTerminal;
use wasmer_api::types::DeployAppVersion;

use crate::{
    commands::{
        app::{deploy_app_verbose, DeployAppOpts, WaitMode},
        AsyncCliCommand,
    },
    opts::{ApiOpts, ItemFormatOpts},
};

/// Start a remote SSH session.
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
        let client = self.api.client()?;

        let base_path = if let Some(p) = self.path {
            p
        } else {
            std::env::current_dir()?
        };
        let file_path = if base_path.is_file() {
            base_path
        } else if base_path.is_dir() {
            let full = base_path.join(AppConfigV1::CANONICAL_FILE_NAME);
            if !full.is_file() {
                bail!("Could not find app.yaml at path: '{}'", full.display());
            }
            full
        } else {
            bail!("No such file or directory: '{}'", base_path.display());
        };
        let abs_dir_path = file_path.canonicalize()?.parent().unwrap().to_owned();

        let interactive = std::io::stdin().is_terminal() && !self.non_interactive;

        let raw_config = std::fs::read_to_string(&file_path)
            .with_context(|| format!("Could not read file: '{}'", file_path.display()))?;

        let orig_config = AppConfigV1::parse_yaml(&raw_config)?;
        eprintln!("Loaded app from: {}", file_path.display());

        // Parse a raw value - will be used later for patching.
        let orig_config_value: serde_yaml::Value =
            serde_yaml::from_str(&raw_config).context("Could not parse app.yaml")?;

        let pkg_name = format!(
            "{}/{}",
            orig_config.package.0.namespace, orig_config.package.0.name
        );

        // Check for a wasmer.toml

        let local_manifest_path = abs_dir_path.join(crate::utils::DEFAULT_PACKAGE_MANIFEST_FILE);
        let local_manifest = crate::utils::load_package_manifest(&local_manifest_path)?
            .map(|x| x.1)
            // Ignore local package if it is not referenced by the app.
            .filter(|m| m.package.name == pkg_name);

        let new_package_manifest = if let Some(manifest) = local_manifest {
            let should_publish = if self.publish_package {
                true
            } else if interactive {
                eprintln!();
                dialoguer::Confirm::new()
                    .with_prompt(format!("Publish new version of package '{}'?", pkg_name))
                    .interact_opt()?
                    .unwrap_or_default()
            } else {
                false
            };

            if should_publish {
                eprintln!("Publishing package...");
                let new_manifest = crate::utils::republish_package_with_bumped_version(
                    &client,
                    &local_manifest_path,
                    manifest,
                )
                .await?;

                eprint!("Waiting for package to become available...");
                std::io::stderr().flush().unwrap();

                let start_wait = std::time::Instant::now();
                loop {
                    if start_wait.elapsed().as_secs() > 300 {
                        bail!("Timed out waiting for package to become available");
                    }

                    eprint!(".");
                    std::io::stderr().flush().unwrap();

                    let new_version_opt = wasmer_api::query::get_package_version(
                        &client,
                        new_manifest.package.name.clone(),
                        new_manifest.package.version.to_string(),
                    )
                    .await;

                    match new_version_opt {
                        Ok(Some(new_version)) => {
                            if new_version.distribution.pirita_sha256_hash.is_some() {
                                eprintln!();
                                break;
                            }
                        }
                        Ok(None) => {
                            bail!("Error - could not query package info: package not found");
                        }
                        Err(e) => {
                            bail!("Error - could not query package info: {e}");
                        }
                    }

                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                }

                eprintln!(
                    "Package '{}@{}' published successfully!",
                    new_manifest.package.name, new_manifest.package.version
                );
                eprintln!();
                Some(new_manifest)
            } else {
                if interactive {
                    eprintln!();
                }
                None
            }
        } else {
            None
        };

        let config = if let Some(manifest) = new_package_manifest {
            let pkg = format!("{}@{}", manifest.package.name, manifest.package.version);
            AppConfigV1 {
                package: pkg.parse()?,
                ..orig_config
            }
        } else {
            orig_config
        };

        let wait_mode = if self.no_wait {
            WaitMode::Deployed
        } else {
            WaitMode::Reachable
        };

        let opts = DeployAppOpts {
            app: &config,
            original_config: Some(orig_config_value.clone()),
            allow_create: true,
            make_default: !self.no_default,
            owner: self.owner,
            wait: wait_mode,
        };
        let (_app, app_version) = deploy_app_verbose(&client, opts).await?;

        let mut new_config = super::app::app_config_from_api(&app_version)?;
        if self.no_persist_id {
            new_config.app_id = None;
        }
        let new_config_value = new_config.to_yaml_value()?;

        // If the config changed, write it back.
        if new_config_value != orig_config_value {
            // We want to preserve unknown fields to allow for newer app.yaml
            // settings without requring new CLI versions, so instead of just
            // serializing the new config, we merge it with the old one.
            let new_merged = crate::utils::merge_yaml_values(&orig_config_value, &new_config_value);
            let new_config_raw = serde_yaml::to_string(&new_merged)?;
            std::fs::write(&file_path, new_config_raw)
                .with_context(|| format!("Could not write file: '{}'", file_path.display()))?;
        }

        if self.fmt.format == crate::utils::render::ItemFormat::Json {
            println!("{}", serde_json::to_string_pretty(&app_version)?);
        }

        Ok(app_version)
    }
}
