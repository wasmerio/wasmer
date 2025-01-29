use super::{util::login_user, AsyncCliCommand};
use crate::{
    commands::{app::create::CmdAppCreate, package::publish::PackagePublish, PublishWait},
    config::WasmerEnv,
    opts::ItemFormatOpts,
    utils::load_package_manifest,
};
use anyhow::Context;
use colored::Colorize;
use dialoguer::Confirm;
use is_terminal::IsTerminal;
use std::io::Write;
use std::{path::PathBuf, str::FromStr, time::Duration};
use wasmer_backend_api::{
    types::{DeployApp, DeployAppVersion},
    WasmerClient,
};
use wasmer_config::{
    app::AppConfigV1,
    package::{PackageIdent, PackageSource},
};

static EDGE_HEADER_APP_VERSION_ID: http::HeaderName =
    http::HeaderName::from_static("x-edge-app-version-id");

/// Deploy an app to Wasmer Edge.
#[derive(clap::Parser, Debug)]
pub struct CmdAppDeploy {
    #[clap(flatten)]
    pub env: WasmerEnv,

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

    /// The path to the directory containing the `app.yaml` file.
    #[clap(long)]
    pub dir: Option<PathBuf>,

    /// The path to the `app.yaml` file.
    #[clap(long, conflicts_with = "dir")]
    pub path: Option<PathBuf>,

    /// Do not wait for the app to become reachable.
    #[clap(long)]
    pub no_wait: bool,

    /// Do not make the new app version the default (active) version.
    /// This is useful for testing a deployment first, before moving it to "production".
    #[clap(long)]
    pub no_default: bool,

    /// Do not persist the app ID under `app_id` field in app.yaml.
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
    #[clap(long, name = "name")]
    pub app_name: Option<String>,

    /// Whether or not to automatically bump the package version if publishing.
    #[clap(long)]
    pub bump: bool,

    /// Don't print any message.
    ///
    /// The only message that will be printed is the one signaling the successfullness of the
    /// operation.
    #[clap(long)]
    pub quiet: bool,

    // - App creation -
    /// A reference to the template to use when creating an app to deploy.
    ///
    /// It can be either an URL to a github repository - like
    /// `https://github.com/wasmer-examples/php-wasmer-starter` -  or the name of a template that
    /// will be searched for in the selected registry, like `astro-starter`.
    #[clap(
        long,
        conflicts_with = "package",
        conflicts_with = "use_local_manifest"
    )]
    pub template: Option<String>,

    /// Name of the package to use when creating an app to deploy.
    #[clap(
        long,
        conflicts_with = "template",
        conflicts_with = "use_local_manifest"
    )]
    pub package: Option<String>,

    /// Whether or not to search (and use) a local manifest when creating an app to deploy.
    #[clap(long, conflicts_with = "template", conflicts_with = "package")]
    pub use_local_manifest: bool,
}

impl CmdAppDeploy {
    async fn publish(
        &self,
        client: &WasmerClient,
        owner: String,
        manifest_dir_path: PathBuf,
    ) -> anyhow::Result<PackageIdent> {
        let (manifest_path, manifest) = match load_package_manifest(&manifest_dir_path)? {
            Some(r) => r,
            None => anyhow::bail!(
                "Could not read or find manifest in path '{}'!",
                manifest_dir_path.display()
            ),
        };

        let publish_cmd = PackagePublish {
            env: self.env.clone(),
            dry_run: false,
            quiet: self.quiet,
            package_name: None,
            package_version: None,
            no_validate: false,
            package_path: manifest_dir_path.clone(),
            wait: match self.no_wait {
                true => PublishWait::None,
                false => PublishWait::Container,
            },
            timeout: humantime::Duration::from_str("2m").unwrap(),
            package_namespace: Some(owner),
            non_interactive: self.non_interactive,
            bump: self.bump,
        };

        publish_cmd
            .publish(client, &manifest_path, &manifest, true)
            .await
    }

    async fn get_owner(
        &self,
        client: &WasmerClient,
        app: &mut serde_yaml::Value,
        maybe_edge_app: Option<&DeployApp>,
    ) -> anyhow::Result<String> {
        if let Some(owner) = &self.owner {
            return Ok(owner.clone());
        }

        if let Some(serde_yaml::Value::String(owner)) = &app.get("owner") {
            return Ok(owner.clone());
        }

        if let Some(edge_app) = maybe_edge_app {
            app.as_mapping_mut()
                .unwrap()
                .insert("owner".into(), edge_app.owner.global_name.clone().into());
            return Ok(edge_app.owner.global_name.clone());
        };

        if self.non_interactive {
            // if not interactive we can't prompt the user to choose the owner of the app.
            anyhow::bail!("No owner specified: use --owner XXX");
        }

        let user = wasmer_backend_api::query::current_user_with_namespaces(client, None).await?;
        let owner = crate::utils::prompts::prompt_for_namespace(
            "Who should own this app?",
            None,
            Some(&user),
        )?;

        app.as_mapping_mut()
            .unwrap()
            .insert("owner".into(), owner.clone().into());

        Ok(owner.clone())
    }
    async fn create(&self) -> anyhow::Result<()> {
        eprintln!("It seems you are trying to create a new app!");

        let create_cmd = CmdAppCreate {
            quiet: self.quiet,
            deploy_app: false,
            no_validate: false,
            non_interactive: false,
            offline: false,
            owner: self.owner.clone(),
            app_name: self.app_name.clone(),
            no_wait: self.no_wait,
            env: self.env.clone(),
            fmt: ItemFormatOpts {
                format: self.fmt.format,
            },
            package: self.package.clone(),
            template: self.template.clone(),
            app_dir_path: self.dir.clone(),
            use_local_manifest: self.use_local_manifest,
            new_package_name: None,
        };

        create_cmd.run_async().await
    }
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppDeploy {
    type Output = ();

    async fn run_async(self) -> Result<Self::Output, anyhow::Error> {
        let client = login_user(&self.env, !self.non_interactive, "deploy an app").await?;

        let base_dir_path = self.dir.clone().unwrap_or_else(|| {
            self.path
                .clone()
                .unwrap_or_else(|| std::env::current_dir().unwrap())
        });

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

        if !app_config_path.is_file()
            || self.template.is_some()
            || self.package.is_some()
            || self.use_local_manifest
        {
            if !self.non_interactive {
                // Create already points back to deploy.
                return self.create().await;
            } else {
                anyhow::bail!("No app configuration was found in {}. Create an app before deploying or re-run in interactive mode!", app_config_path.display());
            }
        }

        assert!(app_config_path.is_file());

        let config_str = std::fs::read_to_string(&app_config_path)
            .with_context(|| format!("Could not read file '{}'", &app_config_path.display()))?;

        // We want to allow the user to specify the app name interactively.
        let mut app_yaml: serde_yaml::Value = serde_yaml::from_str(&config_str)?;
        let maybe_edge_app = if let Some(app_id) = app_yaml.get("app_id").and_then(|s| s.as_str()) {
            wasmer_backend_api::query::get_app_by_id(&client, app_id.to_owned())
                .await
                .ok()
        } else {
            None
        };

        let mut owner = self
            .get_owner(&client, &mut app_yaml, maybe_edge_app.as_ref())
            .await?;

        if !wasmer_backend_api::query::viewer_can_deploy_to_namespace(&client, &owner).await? {
            eprintln!("It seems you don't have access to {}", owner.bold());
            if self.non_interactive {
                anyhow::bail!("Please, change the owner before deploying or check your current user with `{} whoami`.", std::env::args().next().unwrap_or("wasmer".into()));
            } else {
                let user =
                    wasmer_backend_api::query::current_user_with_namespaces(&client, None).await?;
                owner = crate::utils::prompts::prompt_for_namespace(
                    "Who should own this app?",
                    None,
                    Some(&user),
                )?;

                app_yaml
                    .as_mapping_mut()
                    .unwrap()
                    .insert("owner".into(), owner.clone().into());

                if app_yaml.get("app_id").is_some() {
                    app_yaml.as_mapping_mut().unwrap().remove("app_id");
                }

                if app_yaml.get("name").is_some() {
                    app_yaml.as_mapping_mut().unwrap().remove("name");
                }
            }
        }

        if app_yaml.get("name").is_none() && self.app_name.is_some() {
            app_yaml.as_mapping_mut().unwrap().insert(
                "name".into(),
                self.app_name.as_ref().unwrap().to_string().into(),
            );
        } else if app_yaml.get("name").is_none() && maybe_edge_app.is_some() {
            app_yaml.as_mapping_mut().unwrap().insert(
                "name".into(),
                maybe_edge_app
                    .as_ref()
                    .map(|v| v.name.to_string())
                    .unwrap()
                    .into(),
            );
        } else if app_yaml.get("name").is_none() {
            if !self.non_interactive {
                let default_name = std::env::current_dir().ok().and_then(|dir| {
                    dir.file_name()
                        .and_then(|f| f.to_str())
                        .map(|s| s.to_owned())
                });
                let app_name = crate::utils::prompts::prompt_new_app_name(
                    "Enter the name of the app",
                    default_name.as_deref(),
                    &owner,
                    self.env.client().ok().as_ref(),
                )
                .await?;

                app_yaml
                    .as_mapping_mut()
                    .unwrap()
                    .insert("name".into(), app_name.into());
            } else {
                if !self.quiet {
                    eprintln!("The app.yaml does not specify any app name.");
                    eprintln!(
                        "Please, use the --app_name <app_name> to specify the name of the app."
                    );
                }

                anyhow::bail!(
                    "Cannot proceed with the deployment as the app spec in path {} does not have
                    a 'name' field.",
                    app_config_path.display()
                )
            }
        }

        let original_app_config: AppConfigV1 = serde_yaml::from_value(app_yaml.clone())?;
        std::fs::write(
            &app_config_path,
            serde_yaml::to_string(&original_app_config)?,
        )
        .with_context(|| format!("Could not write file: '{}'", app_config_path.display()))?;

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
                let path = PathBuf::from(path);

                let path = if path.is_absolute() {
                    path
                } else {
                    app_config_path.parent().unwrap().join(path)
                };

                if !self.quiet {
                    eprintln!("Loading local package (manifest path: {})", path.display());
                }

                let package_id = self.publish(&client, owner.clone(), path).await?;

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
                        if let Some(name) = &package.name {
                            if name == &n.full_name() {
                                if !self.quiet {
                                    eprintln!(
                                        "Found local package (manifest path: {}).",
                                        manifest_path.display()
                                    );
                                    eprintln!("The `package` field in `app.yaml` specified the same named package ({name}).");
                                    eprintln!("This behaviour is deprecated.");
                                }

                                let theme = dialoguer::theme::ColorfulTheme::default();
                                if self.non_interactive {
                                    if !self.quiet {
                                        eprintln!("Hint: replace `package: {n}` with `package: .` to replicate the intended behaviour.");
                                    }
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

                                    let package_id =
                                        self.publish(&client, owner.clone(), manifest_path).await?;

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
                                    if !self.quiet {
                                        eprintln!(
                                        "{}: the package will not be published and the deployment will fail if the package does not already exist.",
                                        "Warning".yellow().bold());
                                    }
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

        let owner = &opts.owner.clone().or_else(|| opts.app.owner.clone());
        let app = &opts.app;

        let pretty_name = if let Some(owner) = &owner {
            format!(
                "{} ({})",
                app.name
                    .as_ref()
                    .context("App name has to be specified")?
                    .bold(),
                owner.bold()
            )
        } else {
            app.name
                .as_ref()
                .context("App name has to be specified")?
                .bold()
                .to_string()
        };

        if !self.quiet {
            eprintln!("\nDeploying app {pretty_name} to Wasmer Edge...\n");
        }

        let app_version = deploy_app(&client, opts.clone()).await?;

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
            // settings without requiring new CLI versions, so instead of just
            // serializing the new config, we merge it with the old one.
            let new_merged = crate::utils::merge_yaml_values(
                &app_config.clone().to_yaml_value()?,
                &new_app_config.to_yaml_value()?,
            );
            let new_config_raw = serde_yaml::to_string(&new_merged)?;
            std::fs::write(&app_config_path, new_config_raw).with_context(|| {
                format!("Could not write file: '{}'", app_config_path.display())
            })?;
        }

        wait_app(&client, opts.clone(), app_version.clone(), self.quiet).await?;

        if self.fmt.format == Some(crate::utils::render::ItemFormat::Json) {
            println!("{}", serde_json::to_string_pretty(&app_version)?);
        }

        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct DeployAppOpts<'a> {
    pub app: &'a AppConfigV1,
    // Original raw yaml config.
    // Present here to enable forwarding unknown fields to the backend, which
    // preserves forwards-compatibility for schema changes.
    pub original_config: Option<serde_yaml::value::Value>,
    #[allow(dead_code)]
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

    let version = wasmer_backend_api::query::publish_deploy_app(
        client,
        wasmer_backend_api::types::PublishDeployAppVars {
            config: raw_config,
            name: app.name.clone().context("Expected an app name")?.into(),
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
pub async fn wait_app(
    client: &WasmerClient,
    opts: DeployAppOpts<'_>,
    version: DeployAppVersion,
    quiet: bool,
) -> Result<(DeployApp, DeployAppVersion), anyhow::Error> {
    let wait = opts.wait;
    let make_default = opts.make_default;

    let app_id = version
        .app
        .as_ref()
        .context("app field on app version is empty")?
        .id
        .inner()
        .to_string();

    let app = wasmer_backend_api::query::get_app_by_id(client, app_id.clone())
        .await
        .context("could not fetch app from backend")?;

    if !quiet {
        eprintln!(
            "App {} ({}) was successfully deployed 🚀",
            app.name.bold(),
            app.owner.global_name.bold()
        );
        eprintln!("{}", app.url.blue().bold().underline());
        eprintln!();
        eprintln!("→ Unique URL: {}", version.url);
        eprintln!("→ Dashboard:  {}", app.admin_url);
    }

    match wait {
        WaitMode::Deployed => {}
        WaitMode::Reachable => {
            if !quiet {
                eprintln!();
                eprintln!("Waiting for new deployment to become available...");
                eprintln!("(You can safely stop waiting now with CTRL-C)");
            }

            let stderr = std::io::stderr();

            tokio::time::sleep(Duration::from_secs(2)).await;

            let start = tokio::time::Instant::now();
            let client = reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(90))
                // Should not follow redirects.
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap();

            let check_url = if make_default { &app.url } else { &version.url };

            let mut sleep_millis: u64 = 1_000;
            loop {
                let total_elapsed = start.elapsed();
                if total_elapsed > Duration::from_secs(60 * 5) {
                    if !quiet {
                        eprintln!();
                    }
                    anyhow::bail!("\nApp still not reachable after 5 minutes...");
                }

                {
                    let mut lock = stderr.lock();

                    if !quiet {
                        write!(&mut lock, ".").unwrap();
                    }
                    lock.flush().unwrap();
                }

                let request_start = tokio::time::Instant::now();

                tracing::debug!(%check_url, "checking health of app");
                match client.get(check_url).send().await {
                    Ok(res) => {
                        let header = res
                            .headers()
                            .get(&EDGE_HEADER_APP_VERSION_ID)
                            .and_then(|x| x.to_str().ok())
                            .unwrap_or_default();

                        tracing::debug!(
                            %check_url,
                            status=res.status().as_u16(),
                            app_version_header=%header,
                            "app request response received",
                        );

                        if header == version.id.inner() {
                            if !quiet {
                                eprintln!();
                            }
                            if !(res.status().is_success() || res.status().is_redirection()) {
                                eprintln!(
                                    "{}",
                                    format!(
                                        "The app version was deployed correctly, but fails with a non-success status code of {}",
                                        res.status()).yellow()
                                );
                            } else {
                                eprintln!("{} Deployment complete", "𖥔".yellow().bold());
                            }

                            break;
                        }

                        tracing::debug!(
                            current=%header,
                            expected=%version.id.inner(),
                            "app is not at the right version yet",
                        );
                    }
                    Err(err) => {
                        tracing::debug!(?err, "health check request failed");
                    }
                };

                // Increase the sleep time between requests, up
                // to a reasonable maximum.
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
