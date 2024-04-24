use super::AsyncCliCommand;
use crate::{
    commands::{app::create::CmdAppCreate, Publish},
    opts::{ApiOpts, ItemFormatOpts},
    utils::load_package_manifest,
};
use anyhow::Context;
use colored::Colorize;
use dialoguer::Confirm;
use is_terminal::IsTerminal;
use std::io::Write;
use std::{path::PathBuf, str::FromStr, time::Duration};
use wasmer_api::{
    types::{DeployApp, DeployAppVersion},
    WasmerClient,
};
use wasmer_config::{
    app::AppConfigV1,
    package::{PackageIdent, PackageSource},
};
use wasmer_registry::wasmer_env::{WasmerEnv, WASMER_DIR};

/// Deploy an app to Wasmer Edge.
#[derive(clap::Parser, Debug)]
pub struct CmdAppDeploy {
    #[clap(flatten)]
    pub api: ApiOpts,

    #[clap(flatten)]
    pub fmt: ItemFormatOpts,

    /// Skip local schema validation.
    #[clap(long)]
    pub no_validate: bool,

    /// Do not prompt for user input.
    #[clap(long, default_value_t = !std::io::stdin().is_terminal())]
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
    ///
    /// If specified via this flag, the owner will be overridden.  Otherwise, the `app.yaml` is
    /// inspected and, if there is no `owner` field in the spec file, the user will be prompted to
    /// select the correct owner. If no owner is found in non-interactive mode the deployment will
    /// fail.
    #[clap(long)]
    pub owner: Option<String>,

    /// Specify the name (user or namespace) of the app to be deployed.
    ///
    /// If specified via this flag, the app_name will be overridden. Otherwise, the `app.yaml` is
    /// inspected and, if there is no `name` field in the spec file, if running interactive the
    /// user will be prompted to insert an app name, otherwise the deployment will fail.
    #[clap(long)]
    pub app_name: Option<String>,

    /// Whether or not to autobump the package version if publishing.
    #[clap(long)]
    pub autobump: bool,
}

impl CmdAppDeploy {
    async fn publish(
        &self,
        owner: String,
        manifest_dir_path: PathBuf,
    ) -> anyhow::Result<PackageIdent> {
        let (_, manifest) = match load_package_manifest(&manifest_dir_path)? {
            Some(r) => r,
            None => anyhow::bail!(
                "Could not read or find manifest in path '{}'!",
                manifest_dir_path.display()
            ),
        };

        let env = WasmerEnv::new(
            if let Ok(dir) = std::env::var("WASMER_DIR") {
                PathBuf::from(dir)
            } else {
                WASMER_DIR.clone()
            },
            self.api.registry.clone().map(|u| u.to_string().into()),
            self.api.token.clone(),
            None,
        );

        let publish_cmd = Publish {
            env,
            dry_run: false,
            quiet: false,
            package_name: None,
            version: None,
            no_validate: false,
            package_path: Some(manifest_dir_path.to_str().unwrap().to_string()),
            wait: !self.no_wait,
            wait_all: !self.no_wait,
            timeout: humantime::Duration::from_str("2m").unwrap(),
            package_namespace: match manifest.package {
                Some(_) => None,
                None => Some(owner),
            },
            non_interactive: self.non_interactive,
            autobump: self.autobump,
        };

        match publish_cmd.run_async().await? {
            Some(id) => Ok(id),
            None => anyhow::bail!("Error while publishing package. Stopping."),
        }
    }

    async fn get_owner(
        &self,
        app: &serde_yaml::Value,
        app_config_path: &PathBuf,
    ) -> anyhow::Result<(String, String)> {
        let r_ret = serde_yaml::to_string(&app)?;

        if let Some(owner) = &self.owner {
            return Ok((owner.clone(), r_ret));
        }

        if let Some(serde_yaml::Value::String(owner)) = &app.get("owner") {
            return Ok((owner.clone(), r_ret));
        }

        if self.non_interactive {
            // if not interactive we can't prompt the user to choose the owner of the app.
            anyhow::bail!("No owner specified: use --owner XXX");
        }

        match self.api.client() {
            Ok(client) => {
                let user = wasmer_api::query::current_user_with_namespaces(&client, None).await?;
                let owner = crate::utils::prompts::prompt_for_namespace(
                    "Who should own this app?",
                    None,
                    Some(&user),
                )?;

                let new_raw_config = format!("owner: {owner}\n{r_ret}");

                std::fs::write(app_config_path, &new_raw_config).with_context(|| {
                    format!("Could not write file: '{}'", app_config_path.display())
                })?;

                Ok((owner.clone(), new_raw_config))

            }
            Err(e) => anyhow::bail!(
                "Can't determine user info: {e}. Please, user `wasmer login` before deploying an app or use the --owner <owner> flag to signal the owner of the app to deploy."
            ),
        }
    }
    async fn create(&self) -> anyhow::Result<()> {
        eprintln!("It seems you are trying to create a new app!");

        let create_cmd = CmdAppCreate {
            template: None,
            deploy_app: false,
            no_validate: false,
            non_interactive: false,
            offline: false,
            owner: None,
            app_name: None,
            no_wait: false,
            api: self.api.clone(),
            fmt: ItemFormatOpts {
                format: self.fmt.format,
            },
            package: None,
            app_dir_path: None,
            use_local_manifest: false,
            new_package_name: None,
        };

        create_cmd.run_async().await
    }
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppDeploy {
    type Output = ();

    async fn run_async(self) -> Result<Self::Output, anyhow::Error> {
        let client = self
            .api
            .client()
            .with_context(|| "Can't begin deploy flow")?;

        let base_dir_path = self.path.clone().unwrap_or(std::env::current_dir()?);
        let (app_config_path, base_dir_path) = {
            if base_dir_path.is_file() {
                (
                    base_dir_path.clone(),
                    base_dir_path.clone().parent().unwrap().to_path_buf(),
                )
            } else if base_dir_path.is_dir() {
                let f = base_dir_path.join(AppConfigV1::CANONICAL_FILE_NAME);

                (f, base_dir_path.clone())
            } else {
                anyhow::bail!("No such file or directory '{}'", base_dir_path.display());
            }
        };

        if !app_config_path.is_file() {
            if !self.non_interactive {
                // Create already points back to deploy.
                return self.create().await;
            } else {
                anyhow::bail!(
                    "Cannot deploy app as no app.yaml was found in path '{}'",
                    app_config_path.display()
                )
            }
        }

        assert!(app_config_path.is_file());

        let config_str = std::fs::read_to_string(&app_config_path)
            .with_context(|| format!("Could not read file '{}'", &app_config_path.display()))?;

        // We want to allow the user to specify the app name interactively.
        let app_yaml: serde_yaml::Value = serde_yaml::from_str(&config_str)?;
        let (owner, mut config_str) = self.get_owner(&app_yaml, &app_config_path).await?;

        // We want to allow the user to specify the app name interactively.
        let app_yaml: serde_yaml::Value = serde_yaml::from_str(&config_str)?;

        if app_yaml.get("name").is_none() && self.app_name.is_some() {
            config_str = format!("{}\nname: {}", config_str, self.app_name.as_ref().unwrap());
        } else if app_yaml.get("name").is_none() {
            if !self.non_interactive {
                let app_name = crate::utils::prompts::prompt_new_app_name(
                    "Enter the name of the app",
                    None,
                    &owner,
                    self.api.client().ok().as_ref(),
                )
                .await?;

                std::fs::write(
                    &app_config_path,
                    format!("{}name: {}", config_str, app_name),
                )?;

                config_str = std::fs::read_to_string(&app_config_path).with_context(|| {
                    format!("Could not read file '{}'", &app_config_path.display())
                })?;
            } else {
                eprintln!("The app.yaml does not specify any app name.");
                eprintln!("Please, use the --app_name <app_name> to specify the name of the app.");

                anyhow::bail!(
                    "Cannot proceed with the deployment as the app spec in path {} does not have
                    a 'name' field.",
                    app_config_path.display()
                )
            }
        }

        let original_app_config: AppConfigV1 = AppConfigV1::parse_yaml(&config_str)?;
        let mut app_config = original_app_config.clone();

        app_config.owner = Some(owner.clone());

        let wait = if self.no_wait {
            WaitMode::Deployed
        } else {
            WaitMode::Reachable
        };

        let mut app_cfg_new = app_config.clone();
        let opts = match &app_cfg_new.package {
            PackageSource::Path(ref path) => {
                eprintln!(
                    "Loading local package (manifest path: {})",
                    PathBuf::from(path)
                        .canonicalize()?
                        .join("wasmer.toml")
                        .display()
                );

                let package_id = self.publish(owner.clone(), PathBuf::from(path)).await?;

                app_cfg_new.package = package_id.into();

                DeployAppOpts {
                    app: &app_cfg_new,
                    original_config: Some(app_config.clone().to_yaml_value().unwrap()),
                    allow_create: true,
                    make_default: !self.no_default,
                    owner: Some(owner),
                    wait,
                }
            }
            PackageSource::Ident(PackageIdent::Named(n)) => {
                // We need to check if we have a manifest with the same name in the
                // same directory as the `app.yaml`.
                //
                // Release v<insert current version> introduced a breaking change on the
                // deployment flow, and we want old CI to explicitly fail.

                if let Ok(Some((manifest_path, manifest))) = load_package_manifest(&base_dir_path) {
                    if let Some(package) = &manifest.package {
                        if package.name == n.full_name() {
                            eprintln!(
                                "Found local package (manifest path: {}).",
                                manifest_path.display()
                            );
                            eprintln!("The `package` field in `app.yaml` specified the same named package ({}).", package.name);
                            eprintln!("This behaviour is deprecated.");
                            let theme = dialoguer::theme::ColorfulTheme::default();
                            if self.non_interactive {
                                eprintln!("Hint: replace `package: {}` with `package: .` to replicate the intended behaviour.", n);
                                anyhow::bail!("deprecated deploy behaviour")
                            } else if Confirm::with_theme(&theme)
                                .with_prompt("Change package to '.' in app.yaml?")
                                .interact()?
                            {
                                app_config.package = PackageSource::Path(String::from("."));
                                // We have to write it right now.
                                let new_config_raw = serde_yaml::to_string(&app_config)?;
                                std::fs::write(&app_config_path, new_config_raw).with_context(
                                    || {
                                        format!(
                                            "Could not write file: '{}'",
                                            app_config_path.display()
                                        )
                                    },
                                )?;

                                log::info!(
                                    "Using package {} ({})",
                                    app_config.package,
                                    n.full_name()
                                );

                                let package_id = self.publish(owner.clone(), manifest_path).await?;

                                app_config.package = package_id.into();

                                DeployAppOpts {
                                    app: &app_config,
                                    original_config: Some(
                                        app_config.clone().to_yaml_value().unwrap(),
                                    ),
                                    allow_create: true,
                                    make_default: !self.no_default,
                                    owner: Some(owner),
                                    wait,
                                }
                            } else {
                                eprintln!(
                                        "{}: the package will not be published and the deployment will fail if the package does not already exist.",
                                        "Warning".yellow().bold()
                                    );
                                DeployAppOpts {
                                    app: &app_config,
                                    original_config: Some(
                                        app_config.clone().to_yaml_value().unwrap(),
                                    ),
                                    allow_create: true,
                                    make_default: !self.no_default,
                                    owner: Some(owner),
                                    wait,
                                }
                            }
                        } else {
                            DeployAppOpts {
                                app: &app_config,
                                original_config: Some(app_config.clone().to_yaml_value().unwrap()),
                                allow_create: true,
                                make_default: !self.no_default,
                                owner: Some(owner),
                                wait,
                            }
                        }
                    } else {
                        DeployAppOpts {
                            app: &app_config,
                            original_config: Some(app_config.clone().to_yaml_value().unwrap()),
                            allow_create: true,
                            make_default: !self.no_default,
                            owner: Some(owner),
                            wait,
                        }
                    }
                } else {
                    log::info!("Using package {}", app_config.package.to_string());
                    DeployAppOpts {
                        app: &app_config,
                        original_config: Some(app_config.clone().to_yaml_value().unwrap()),
                        allow_create: true,
                        make_default: !self.no_default,
                        owner: Some(owner),
                        wait,
                    }
                }
            }
            _ => {
                log::info!("Using package {}", app_config.package.to_string());
                DeployAppOpts {
                    app: &app_config,
                    original_config: Some(app_config.clone().to_yaml_value().unwrap()),
                    allow_create: true,
                    make_default: !self.no_default,
                    owner: Some(owner),
                    wait,
                }
            }
        };

        let (_app, app_version) = deploy_app_verbose(&client, opts).await?;

        let mut new_app_config = app_config_from_api(&app_version)?;

        if self.no_persist_id {
            new_app_config.app_id = None;
        }

        // Don't override the package field.
        new_app_config.package = app_config.package.clone();
        // [TODO]: check if name was added...

        // If the config changed, write it back.
        if new_app_config != app_config {
            // We want to preserve unknown fields to allow for newer app.yaml
            // settings without requring new CLI versions, so instead of just
            // serializing the new config, we merge it with the old one.
            let new_merged = crate::utils::merge_yaml_values(
                &app_config.to_yaml_value()?,
                &new_app_config.to_yaml_value()?,
            );
            let new_config_raw = serde_yaml::to_string(&new_merged)?;
            std::fs::write(&app_config_path, new_config_raw).with_context(|| {
                format!("Could not write file: '{}'", app_config_path.display())
            })?;
        }

        if self.fmt.format == crate::utils::render::ItemFormat::Json {
            println!("{}", serde_json::to_string_pretty(&app_version)?);
        }

        Ok(())
    }
}

#[derive(Debug)]
pub struct DeployAppOpts<'a> {
    pub app: &'a AppConfigV1,
    // Original raw yaml config.
    // Present here to enable forwarding unknown fields to the backend, which
    // preserves forwards-compatibility for schema changes.
    pub original_config: Option<serde_yaml::value::Value>,
    pub allow_create: bool,
    pub make_default: bool,
    pub owner: Option<String>,
    pub wait: WaitMode,
}

pub async fn deploy_app(
    client: &WasmerClient,
    opts: DeployAppOpts<'_>,
) -> Result<DeployAppVersion, anyhow::Error> {
    let app = opts.app;

    let config_value = app.clone().to_yaml_value()?;
    let final_config = if let Some(old) = &opts.original_config {
        crate::utils::merge_yaml_values(old, &config_value)
    } else {
        config_value
    };
    let mut raw_config = serde_yaml::to_string(&final_config)?.trim().to_string();
    raw_config.push('\n');

    // TODO: respect allow_create flag

    let version = wasmer_api::query::publish_deploy_app(
        client,
        wasmer_api::types::PublishDeployAppVars {
            config: raw_config,
            name: app.name.clone().into(),
            owner: opts.owner.map(|o| o.into()),
            make_default: Some(opts.make_default),
        },
    )
    .await
    .context("could not create app in the backend")?;

    Ok(version)
}

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum WaitMode {
    /// Wait for the app to be deployed.
    Deployed,
    /// Wait for the app to be deployed and ready.
    Reachable,
}

/// Same as [Self::deploy], but also prints verbose information.
pub async fn deploy_app_verbose(
    client: &WasmerClient,
    opts: DeployAppOpts<'_>,
) -> Result<(DeployApp, DeployAppVersion), anyhow::Error> {
    let owner = &opts.owner.clone().or_else(|| opts.app.owner.clone());
    let app = &opts.app;

    let pretty_name = if let Some(owner) = &owner {
        format!("{} ({})", app.name.bold(), owner.bold())
    } else {
        app.name.bold().to_string()
    };

    let make_default = opts.make_default;

    eprintln!("Deploying app {} to Wasmer Edge...\n", pretty_name);

    let wait = opts.wait;
    let version = deploy_app(client, opts).await?;

    let app_id = version
        .app
        .as_ref()
        .context("app field on app version is empty")?
        .id
        .inner()
        .to_string();

    let app = wasmer_api::query::get_app_by_id(client, app_id.clone())
        .await
        .context("could not fetch app from backend")?;

    eprintln!(
        "App {} ({}) was successfully deployed 🚀",
        app.name.bold(),
        app.owner.global_name.bold()
    );
    eprintln!("{}", app.url.blue().bold().underline());
    eprintln!();
    eprintln!("→ Unique URL: {}", version.url);
    eprintln!("→ Dashboard:  {}", app.admin_url);

    match wait {
        WaitMode::Deployed => {}
        WaitMode::Reachable => {
            eprintln!();
            eprintln!("Waiting for new deployment to become available...");
            eprintln!("(You can safely stop waiting now with CTRL-C)");

            let stderr = std::io::stderr();

            tokio::time::sleep(Duration::from_secs(2)).await;

            let start = tokio::time::Instant::now();
            let client = reqwest::Client::new();

            let check_url = if make_default { &app.url } else { &version.url };

            let mut sleep_millis: u64 = 1_000;
            loop {
                let total_elapsed = start.elapsed();
                if total_elapsed > Duration::from_secs(60 * 5) {
                    eprintln!();
                    anyhow::bail!("\nApp still not reachable after 5 minutes...");
                }

                {
                    let mut lock = stderr.lock();
                    write!(&mut lock, ".").unwrap();
                    lock.flush().unwrap();
                }

                let request_start = tokio::time::Instant::now();

                match client.get(check_url).send().await {
                    Ok(res) => {
                        let header = res
                            .headers()
                            .get(edge_util::headers::HEADER_APP_VERSION_ID)
                            .and_then(|x| x.to_str().ok())
                            .unwrap_or_default();

                        if header == version.id.inner() {
                            eprintln!("\nNew version is now reachable at {check_url}");
                            eprintln!("✅ Deployment complete");
                            break;
                        }

                        tracing::debug!(
                            current=%header,
                            expected=%app.active_version.id.inner(),
                            "app is not at the right version yet",
                        );
                    }
                    Err(err) => {
                        tracing::debug!(?err, "health check request failed");
                    }
                };

                let elapsed: u64 = request_start
                    .elapsed()
                    .as_millis()
                    .try_into()
                    .unwrap_or_default();
                let to_sleep = Duration::from_millis(sleep_millis.saturating_sub(elapsed));
                tokio::time::sleep(to_sleep).await;
                sleep_millis = (sleep_millis * 2).max(10_000);
            }
        }
    }

    Ok((app, version))
}

pub fn app_config_from_api(version: &DeployAppVersion) -> Result<AppConfigV1, anyhow::Error> {
    let app_id = version
        .app
        .as_ref()
        .context("app field on app version is empty")?
        .id
        .inner()
        .to_string();

    let cfg = &version.user_yaml_config;
    let mut cfg = AppConfigV1::parse_yaml(cfg)
        .context("could not parse app config from backend app version")?;

    cfg.app_id = Some(app_id);
    Ok(cfg)
}
