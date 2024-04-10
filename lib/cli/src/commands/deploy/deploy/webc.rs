use super::Deployable;
use crate::commands::{
    app::{DeployAppOpts, WaitMode},
    deploy::CmdDeploy,
};
use anyhow::Context;
use edge_schema::schema::{AppConfigV1, StringWebcIdent};
use is_terminal::IsTerminal;
use std::{io::Write, path::PathBuf};
use wasmer_api::types::DeployAppVersion;

#[async_trait::async_trait]
impl Deployable for StringWebcIdent {
    async fn deploy(
        &self,
        app_config_path: PathBuf,
        config: &AppConfigV1,
        cmd: &CmdDeploy,
    ) -> anyhow::Result<DeployAppVersion> {
        let webc_id = self.0;
        let client = cmd.api.client()?;
        let pkg_name = format!("{}/{}", webc_id.namespace, webc_id.name);
        let interactive = std::io::stdin().is_terminal() && !cmd.non_interactive;
        let dir_path = app_config_path.canonicalize()?.parent().unwrap().to_owned();

        // Find and load the mandatory `wasmer.toml` file.
        let local_manifest_path = dir_path.join(crate::utils::DEFAULT_PACKAGE_MANIFEST_FILE);
        let local_manifest = crate::utils::load_package_manifest(&local_manifest_path)?
            .map(|x| x.1)
            // Ignore local package if it is not referenced by the app.
            .filter(|m| m.package.name == pkg_name);

        let new_package_manifest = if let Some(manifest) = local_manifest {
            let should_publish = if cmd.publish_package {
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
                        anyhow::bail!("Timed out waiting for package to become available");
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
                            anyhow::bail!(
                                "Error - could not query package info: package not found"
                            );
                        }
                        Err(e) => {
                            anyhow::bail!("Error - could not query package info: {e}");
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
                ..config.clone()
            }
        } else {
            config.clone()
        };

        let wait_mode = if cmd.no_wait {
            WaitMode::Deployed
        } else {
            WaitMode::Reachable
        };

        let opts = DeployAppOpts {
            app: &config,
            original_config: Some(config.clone().to_yaml_value().unwrap()),
            allow_create: true,
            make_default: !cmd.no_default,
            owner: cmd.owner,
            wait: wait_mode,
        };
        let (_app, app_version) = crate::commands::app::deploy_app_verbose(&client, opts).await?;

        let mut new_config = crate::commands::app::app_config_from_api(&app_version)?;
        if cmd.no_persist_id {
            new_config.app_id = None;
        }
        // If the config changed, write it back.
        if new_config != config {
            // We want to preserve unknown fields to allow for newer app.yaml
            // settings without requring new CLI versions, so instead of just
            // serializing the new config, we merge it with the old one.
            let new_merged = crate::utils::merge_yaml_values(
                &config.to_yaml_value()?,
                &new_config.to_yaml_value()?,
            );
            let new_config_raw = serde_yaml::to_string(&new_merged)?;
            std::fs::write(&app_config_path, new_config_raw).with_context(|| {
                format!("Could not write file: '{}'", app_config_path.display())
            })?;
        }

        if cmd.fmt.format == crate::utils::render::ItemFormat::Json {
            println!("{}", serde_json::to_string_pretty(&app_version)?);
        }

        Ok(app_version)
    }
}
