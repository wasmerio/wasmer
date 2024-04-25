//! Create a new Edge app.

use crate::{
    commands::AsyncCliCommand,
    opts::{ApiOpts, ItemFormatOpts},
    utils::{
        load_package_manifest,
        package_wizard::{CreateMode, PackageType, PackageWizard},
    },
};
use anyhow::Context;
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Confirm, Select};
use is_terminal::IsTerminal;
use std::{collections::HashMap, env, path::PathBuf, str::FromStr};
use wasmer_api::{types::UserWithNamespaces, WasmerClient};
use wasmer_config::{
    app::AppConfigV1,
    package::{NamedPackageIdent, PackageSource, Tag},
};

use super::deploy::CmdAppDeploy;

async fn write_app_config(app_config: &AppConfigV1, dir: Option<PathBuf>) -> anyhow::Result<()> {
    let raw_app_config = app_config.clone().to_yaml()?;

    let app_dir = match dir {
        Some(dir) => dir,
        None => std::env::current_dir()?,
    };

    let app_config_path = app_dir.join(AppConfigV1::CANONICAL_FILE_NAME);
    std::fs::write(&app_config_path, raw_app_config).with_context(|| {
        format!(
            "could not write app config to '{}'",
            app_config_path.display()
        )
    })
}

/// Create a new Edge app.
#[derive(clap::Parser, Debug)]
pub struct CmdAppCreate {
    #[clap(name = "type", short = 't', long)]
    pub template: Option<AppType>,
    /// Whether or not to deploy the application once it is created.
    ///
    /// If selected, this might entail the step of publishing the package related to the
    /// application. By default, the application is not deployed and the package is not published.
    #[clap(long)]
    pub deploy_app: bool,

    /// Skip local schema validation.
    #[clap(long)]
    pub no_validate: bool,

    /// Do not prompt for user input.
    #[clap(long, default_value_t = !std::io::stdin().is_terminal())]
    pub non_interactive: bool,

    /// Do not interact with any APIs.
    #[clap(long)]
    pub offline: bool,

    /// The owner of the app.
    #[clap(long)]
    pub owner: Option<String>,

    /// The name of the app (can be changed later)
    #[clap(long = "name")]
    pub app_name: Option<String>,

    /// The path to the directory where the config file for the application will be written to.
    #[clap(long = "path")]
    pub app_dir_path: Option<PathBuf>,

    /// Do not wait for the app to become reachable if deployed.
    #[clap(long)]
    pub no_wait: bool,

    // Common args.
    #[clap(flatten)]
    #[allow(missing_docs)]
    pub api: ApiOpts,

    #[clap(flatten)]
    #[allow(missing_docs)]
    pub fmt: ItemFormatOpts,

    /// Name of the package to use.
    #[clap(long, short = 'p')]
    pub package: Option<String>,

    /// Whether or not to search (and use) a local manifest.
    #[clap(long)]
    pub use_local_manifest: bool,

    /// Name to use when creating a new package from a template.
    #[clap(long)]
    pub new_package_name: Option<String>,
}

impl CmdAppCreate {
    #[inline]
    fn get_app_config(&self, owner: &str, name: &str, package: &str) -> AppConfigV1 {
        AppConfigV1 {
            name: String::from(name),
            owner: Some(String::from(owner)),
            package: PackageSource::from_str(package).unwrap(),
            app_id: None,
            domains: None,
            env: HashMap::new(),
            cli_args: None,
            capabilities: None,
            scheduled_tasks: None,
            volumes: None,
            health_checks: None,
            debug: None,
            scaling: None,
            extra: HashMap::new(),
        }
    }

    async fn get_app_name(&self) -> anyhow::Result<String> {
        if let Some(name) = &self.app_name {
            return Ok(name.clone());
        }

        if self.non_interactive {
            // if not interactive we can't prompt the user to choose the owner of the app.
            anyhow::bail!("No app name specified: use --name <app_name>");
        }

        let default_name = env::current_dir().ok().and_then(|dir| {
            dir.file_name()
                .and_then(|f| f.to_str())
                .map(|s| s.to_owned())
        });
        crate::utils::prompts::prompt_for_ident(
            "What should be the name of the app?",
            default_name.as_deref(),
        )
    }

    async fn get_owner(&self) -> anyhow::Result<String> {
        if let Some(owner) = &self.owner {
            return Ok(owner.clone());
        }

        if self.non_interactive {
            // if not interactive we can't prompt the user to choose the owner of the app.
            anyhow::bail!("No owner specified: use --owner <owner>");
        }

        if !self.offline {
            match self.api.client() {
                Ok(client) => {
                    let user =
                        wasmer_api::query::current_user_with_namespaces(&client, None).await?;
                    crate::utils::prompts::prompt_for_namespace(
                        "Who should own this app?",
                        None,
                        Some(&user),
                    )
                }
                Err(e) => anyhow::bail!(
                "Can't determine user info: {e}. Please, user `wasmer login` before deploying an
                app or use the --owner <owner> flag to specify the owner of the app to deploy."
            ),
            }
        } else {
            anyhow::bail!(
                "Please, user `wasmer login` before deploying an app or use the --owner <owner>
                flag to specify the owner of the app to deploy."
            )
        }
    }

    async fn create_from_local_manifest(
        &self,
        owner: &str,
        app_name: &str,
    ) -> anyhow::Result<bool> {
        if !self.use_local_manifest && self.non_interactive {
            return Ok(false);
        }

        let app_dir = match &self.app_dir_path {
            Some(dir) => PathBuf::from(dir),
            None => std::env::current_dir()?,
        };

        let (manifest_path, _) = if let Some(res) = load_package_manifest(&app_dir)? {
            res
        } else if self.use_local_manifest {
            anyhow::bail!("The --use_local_manifest flag was passed, but path {} does not contain a valid package manifest.", app_dir.display())
        } else {
            return Ok(false);
        };

        let ask_confirmation = || {
            eprintln!(
                "A package manifest was found in path {}.",
                &manifest_path.display()
            );
            let theme = dialoguer::theme::ColorfulTheme::default();
            Confirm::with_theme(&theme)
                .with_prompt("Use it for the app?")
                .interact()
        };

        if self.use_local_manifest || ask_confirmation()? {
            let app_config =
                self.get_app_config(owner, app_name, manifest_path.to_string_lossy().as_ref());
            write_app_config(&app_config, self.app_dir_path.clone()).await?;
            self.try_deploy(owner).await?;
            return Ok(true);
        }

        Ok(false)
    }

    async fn create_from_package(&self, owner: &str, app_name: &str) -> anyhow::Result<bool> {
        if self.template.is_some() {
            return Ok(false);
        }

        if let Some(pkg) = &self.package {
            let app_config = self.get_app_config(owner, app_name, pkg);
            write_app_config(&app_config, self.app_dir_path.clone()).await?;
            self.try_deploy(owner).await?;
            return Ok(true);
        } else if !self.non_interactive {
            let theme = ColorfulTheme::default();
            let package_name: String = dialoguer::Input::with_theme(&theme)
                .with_prompt("What is the name of the package?")
                .interact()?;

            let app_config = self.get_app_config(owner, app_name, &package_name);
            write_app_config(&app_config, self.app_dir_path.clone()).await?;
            self.try_deploy(owner).await?;
            return Ok(true);
        } else {
            eprintln!(
                "{}: the app creation process did not produce any local change.",
                "Warning".bold().yellow()
            );
        }

        Ok(false)
    }

    async fn create_from_template(&self, owner: &str, app_name: &str) -> anyhow::Result<bool> {
        let template = match self.template {
            Some(t) => t,
            None => {
                if !self.non_interactive {
                    let theme = ColorfulTheme::default();
                    let index = dialoguer::Select::with_theme(&theme)
                        .with_prompt("App type")
                        .default(0)
                        .items(&[
                            "Static website",
                            "HTTP server",
                            "Browser shell",
                            "JS Worker (WinterJS)",
                            "Python Application",
                        ])
                        .interact()?;
                    match index {
                        0 => AppType::StaticWebsite,
                        1 => AppType::HttpServer,
                        2 => AppType::BrowserShell,
                        3 => AppType::JsWorker,
                        4 => AppType::PyApplication,
                        x => panic!("unhandled app type index '{x}'"),
                    }
                } else {
                    return Ok(false);
                }
            }
        };

        let allow_local_package = match template {
            AppType::HttpServer => true,
            AppType::StaticWebsite => true,
            AppType::BrowserShell => false,
            AppType::JsWorker => true,
            AppType::PyApplication => true,
        };

        let app_dir_path = match &self.app_dir_path {
            Some(dir) => dir.clone(),
            None => std::env::current_dir()?,
        };

        let local_package = if allow_local_package {
            match crate::utils::load_package_manifest(&app_dir_path) {
                Ok(Some(p)) => Some(p),
                Ok(None) => None,
                Err(err) => {
                    eprintln!(
                        "{warning}: could not load package manifest: {err}",
                        warning = "Warning".yellow(),
                    );
                    None
                }
            }
        } else {
            None
        };

        let user = if self.offline {
            None
        } else if let Ok(client) = &self.api.client() {
            let u = wasmer_api::query::current_user_with_namespaces(
                client,
                Some(wasmer_api::types::GrapheneRole::Admin),
            )
            .await?;
            Some(u)
        } else {
            None
        };
        let creator = AppCreator {
            app_name: String::from(app_name),
            new_package_name: self.new_package_name.clone(),
            package: self.package.clone(),
            template,
            interactive: !self.non_interactive,
            app_dir_path,
            owner: String::from(owner),
            api: if self.offline {
                None
            } else {
                self.api.client().ok()
            },
            user,
            local_package,
        };

        match template {
            AppType::HttpServer
            | AppType::StaticWebsite
            | AppType::JsWorker
            | AppType::PyApplication => creator.build_app().await?,
            AppType::BrowserShell => creator.build_browser_shell_app().await?,
        };

        self.try_deploy(owner).await?;

        Ok(true)
    }

    async fn try_deploy(&self, owner: &str) -> anyhow::Result<()> {
        let interactive = !self.non_interactive;
        let theme = dialoguer::theme::ColorfulTheme::default();

        if self.deploy_app
            || (interactive
                && Confirm::with_theme(&theme)
                    .with_prompt("Do you want to deploy the app now?")
                    .interact()?)
        {
            let cmd_deploy = CmdAppDeploy {
                api: self.api.clone(),
                fmt: ItemFormatOpts {
                    format: self.fmt.format,
                },
                no_validate: false,
                non_interactive: self.non_interactive,
                publish_package: true,
                path: self.app_dir_path.clone(),
                no_wait: false,
                no_default: false,
                no_persist_id: false,
                owner: Some(String::from(owner)),
                app_name: None,
                autobump: false,
            };
            cmd_deploy.run_async().await?;
        }

        Ok(())
    }
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppCreate {
    type Output = ();

    async fn run_async(self) -> Result<Self::Output, anyhow::Error> {
        // Get the future owner of the app.
        let owner = self.get_owner().await?;

        // Get the name of the app.
        let app_name = self.get_app_name().await?;

        if !self.create_from_local_manifest(&owner, &app_name).await? {
            if self.template.is_some() {
                self.create_from_template(&owner, &app_name).await?;
            } else if self.package.is_some() {
                self.create_from_package(&owner, &app_name).await?;
            } else if !self.non_interactive {
                let theme = ColorfulTheme::default();
                let choice = Select::with_theme(&theme)
                    .with_prompt("What would you like to deploy?")
                    .items(&["Start with a template", "Choose an existing package"])
                    .default(0)
                    .interact()?;
                match choice {
                    0 => self.create_from_template(&owner, &app_name).await?,
                    1 => self.create_from_package(&owner, &app_name).await?,
                    x => panic!("unhandled selection {x}"),
                };
            } else {
                eprintln!("Warning: the creation process did not produce any result.");
            }
        }

        Ok(())
    }
}

/// App type.
#[derive(clap::ValueEnum, Clone, Copy, Debug)]
pub enum AppType {
    /// A HTTP server.
    #[clap(name = "http")]
    HttpServer,
    /// A static website.
    #[clap(name = "static-website")]
    StaticWebsite,
    /// Wraps another package to run in the browser.
    #[clap(name = "browser-shell")]
    BrowserShell,
    /// Winter-js based JS-Worker
    #[clap(name = "js-worker")]
    JsWorker,
    /// Python worker
    #[clap(name = "py-application")]
    PyApplication,
}

struct AppCreator {
    package: Option<String>,
    new_package_name: Option<String>,
    app_name: String,
    template: AppType,
    interactive: bool,
    app_dir_path: PathBuf,
    owner: String,
    api: Option<WasmerClient>,
    user: Option<UserWithNamespaces>,
    local_package: Option<(PathBuf, wasmer_config::package::Manifest)>,
}

impl AppCreator {
    async fn build_browser_shell_app(self) -> Result<(), anyhow::Error> {
        const WASM_BROWSER_CONTAINER_PACKAGE: &str = "wasmer/wasmer-sh";
        const WASM_BROWSER_CONTAINER_VERSION: &str = "0.2";

        eprintln!("A browser web shell wraps another package and runs it in the browser");
        eprintln!("Select the package to wrap.");

        let (inner_pkg, _inner_pkg_api) = crate::utils::prompt_for_package(
            "Package",
            None,
            Some(crate::utils::PackageCheckMode::MustExist),
            self.api.as_ref(),
        )
        .await?;

        let app_name = self.app_name;
        eprintln!("What should be the name of the package?");

        let default_name = format!(
            "{}-{}-webshell",
            self.owner,
            inner_pkg.to_string().replace('/', "-")
        );

        let outer_pkg_name =
            crate::utils::prompts::prompt_for_ident("Package name", Some(&default_name))?;
        let outer_pkg_full_name = format!("{}/{}", self.owner, outer_pkg_name);

        // Build the package.

        let public_dir = self.app_dir_path.join("public");
        if !public_dir.exists() {
            std::fs::create_dir_all(&public_dir)?;
        }

        let init = serde_json::json!({
            "init": format!("{}/{}", inner_pkg.namespace.as_ref().unwrap(), inner_pkg.name),
            "prompt": inner_pkg.name,
            "no_welcome": true,
            "connect": format!("wss://{app_name}.wasmer.app/.well-known/edge-vpn"),
        });
        let init_path = public_dir.join("init.json");
        std::fs::write(&init_path, init.to_string())
            .with_context(|| format!("Failed to write to '{}'", init_path.display()))?;

        let package = wasmer_config::package::PackageBuilder::new(
            outer_pkg_full_name,
            "0.1.0".parse().unwrap(),
            format!("{} web shell", inner_pkg.name),
        )
        .rename_commands_to_raw_command_name(false)
        .build()?;

        let manifest = wasmer_config::package::ManifestBuilder::new(package)
            .with_dependency(
                WASM_BROWSER_CONTAINER_PACKAGE,
                WASM_BROWSER_CONTAINER_VERSION.to_string().parse().unwrap(),
            )
            .map_fs("public", PathBuf::from("public"))
            .build()?;

        let manifest_path = self.app_dir_path.join("wasmer.toml");

        let raw = manifest.to_string()?;
        eprintln!(
            "Writing wasmer.toml package to '{}'",
            manifest_path.display()
        );
        std::fs::write(&manifest_path, raw)?;

        let app_config = AppConfigV1 {
            name: app_name,
            app_id: None,
            owner: Some(self.owner.clone()),
            package: PackageSource::Path(".".into()),
            domains: None,
            env: Default::default(),
            cli_args: None,
            capabilities: None,
            scheduled_tasks: None,
            volumes: None,
            health_checks: None,
            debug: Some(false),
            scaling: None,
            extra: Default::default(),
        };

        write_app_config(&app_config, Some(self.app_dir_path.clone())).await?;

        Ok(())
    }

    async fn build_app(self) -> Result<(), anyhow::Error> {
        let package_opt: Option<NamedPackageIdent> = if let Some(package) = self.package {
            Some(NamedPackageIdent::from_str(&package)?)
        } else if let Some((_, local)) = self.local_package.as_ref() {
            let pkg = match &local.package {
                Some(pkg) => pkg.clone(),
                None => anyhow::bail!(
                    "Error while building app: template manifest has no package field!"
                ),
            };

            let full = format!("{}@{}", pkg.name, pkg.version);
            let mut pkg_ident = NamedPackageIdent::from_str(&pkg.name)
                .with_context(|| format!("local package manifest has invalid name: '{full}'"))?;

            // Pin the version.
            pkg_ident.tag = Some(Tag::from_str(&pkg.version.to_string()).unwrap());

            if self.interactive {
                eprintln!("Found local package: '{}'", full.green());

                let msg = format!("Use package '{pkg_ident}'");

                let theme = dialoguer::theme::ColorfulTheme::default();
                let should_use = Confirm::with_theme(&theme)
                    .with_prompt(&msg)
                    .interact_opt()?
                    .unwrap_or_default();

                if should_use {
                    Some(pkg_ident)
                } else {
                    None
                }
            } else {
                Some(pkg_ident)
            }
        } else {
            None
        };

        let (package, _api_pkg, _local_package) = if let Some(pkg) = package_opt {
            if let Some(api) = &self.api {
                let p2 = wasmer_api::query::get_package(
                    api,
                    format!("{}/{}", pkg.namespace.as_ref().unwrap(), pkg.name),
                )
                .await?;

                (
                    PackageSource::Ident(wasmer_config::package::PackageIdent::Named(pkg)),
                    p2,
                    self.local_package,
                )
            } else {
                (
                    PackageSource::Ident(wasmer_config::package::PackageIdent::Named(pkg)),
                    None,
                    self.local_package,
                )
            }
        } else {
            let ty = match self.template {
                AppType::HttpServer => None,
                AppType::StaticWebsite => Some(PackageType::StaticWebsite),
                AppType::BrowserShell => None,
                AppType::JsWorker => Some(PackageType::JsWorker),
                AppType::PyApplication => Some(PackageType::PyApplication),
            };

            let create_mode = match ty {
                Some(PackageType::StaticWebsite)
                | Some(PackageType::JsWorker)
                | Some(PackageType::PyApplication) => CreateMode::Create,
                // Only static website creation is currently supported.
                _ => CreateMode::SelectExisting,
            };

            let w = PackageWizard {
                path: self.app_dir_path.clone(),
                name: self.new_package_name.clone(),
                type_: ty,
                create_mode,
                namespace: Some(self.owner.clone()),
                namespace_default: self.user.as_ref().map(|u| u.username.clone()),
                user: self.user.clone(),
            };

            let output = w.run(self.api.as_ref()).await?;
            (
                PackageSource::Path(".".into()),
                output.api,
                output
                    .local_path
                    .and_then(move |x| Some((x, output.local_manifest?))),
            )
        };

        let name = self.app_name;

        let cli_args = match self.template {
            AppType::PyApplication => Some(vec!["/src/main.py".to_string()]),
            AppType::JsWorker => Some(vec!["/src/index.js".to_string()]),
            _ => None,
        };

        // TODO: check if name already exists.
        let app_config = AppConfigV1 {
            name,
            app_id: None,
            owner: Some(self.owner.clone()),
            package,
            domains: None,
            env: Default::default(),
            // CLI args are only set for JS and Py workers for now.
            cli_args,
            // TODO: allow setting the description.
            // description: Some("".to_string()),
            capabilities: None,
            scheduled_tasks: None,
            volumes: None,
            health_checks: None,
            debug: Some(false),
            scaling: None,
            extra: Default::default(),
        };

        write_app_config(&app_config, Some(self.app_dir_path.clone())).await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_app_create_static_site_offline() {
        let dir = tempfile::tempdir().unwrap();

        let cmd = CmdAppCreate {
            template: Some(AppType::StaticWebsite),
            deploy_app: false,
            no_validate: false,
            non_interactive: true,
            offline: true,
            owner: Some("testuser".to_string()),
            app_name: Some("static-site-1".to_string()),
            app_dir_path: Some(dir.path().to_owned()),
            no_wait: true,
            api: ApiOpts::default(),
            fmt: ItemFormatOpts::default(),
            package: Some("testuser/static-site-1@0.1.0".to_string()),
            use_local_manifest: false,
            new_package_name: None,
        };
        cmd.run_async().await.unwrap();

        let app = std::fs::read_to_string(dir.path().join("app.yaml")).unwrap();
        assert_eq!(
            app,
            r#"kind: wasmer.io/App.v0
name: static-site-1
owner: testuser
package: testuser/static-site-1@^0.1.0
debug: false
"#,
        );
    }

    #[tokio::test]
    async fn test_app_create_offline_with_package() {
        let dir = tempfile::tempdir().unwrap();

        let cmd = CmdAppCreate {
            template: Some(AppType::HttpServer),
            deploy_app: false,
            no_validate: false,
            non_interactive: true,
            offline: true,
            owner: Some("wasmer".to_string()),
            app_name: Some("testapp".to_string()),
            app_dir_path: Some(dir.path().to_owned()),
            no_wait: true,
            api: ApiOpts::default(),
            fmt: ItemFormatOpts::default(),
            package: Some("wasmer/testpkg".to_string()),
            use_local_manifest: false,
            new_package_name: None,
        };
        cmd.run_async().await.unwrap();

        let app = std::fs::read_to_string(dir.path().join("app.yaml")).unwrap();
        assert_eq!(
            app,
            r#"kind: wasmer.io/App.v0
name: testapp
owner: wasmer
package: wasmer/testpkg
debug: false
"#,
        );
    }
    #[tokio::test]
    async fn test_app_create_js_worker() {
        let dir = tempfile::tempdir().unwrap();

        let cmd = CmdAppCreate {
            template: Some(AppType::JsWorker),
            deploy_app: false,
            no_validate: false,
            non_interactive: true,
            offline: true,
            owner: Some("wasmer".to_string()),
            app_name: Some("test-js-worker".to_string()),
            app_dir_path: Some(dir.path().to_owned()),
            no_wait: true,
            api: ApiOpts::default(),
            fmt: ItemFormatOpts::default(),
            package: Some("wasmer/test-js-worker".to_string()),
            use_local_manifest: false,
            new_package_name: None,
        };
        cmd.run_async().await.unwrap();

        let app = std::fs::read_to_string(dir.path().join("app.yaml")).unwrap();
        assert_eq!(
            app,
            r#"kind: wasmer.io/App.v0
name: test-js-worker
owner: wasmer
package: wasmer/test-js-worker
cli_args:
- /src/index.js
debug: false
"#,
        );
    }

    #[tokio::test]
    async fn test_app_create_py_worker() {
        let dir = tempfile::tempdir().unwrap();

        let cmd = CmdAppCreate {
            template: Some(AppType::PyApplication),
            deploy_app: false,
            no_validate: false,
            non_interactive: true,
            offline: true,
            owner: Some("wasmer".to_string()),
            app_name: Some("test-py-worker".to_string()),
            app_dir_path: Some(dir.path().to_owned()),
            no_wait: true,
            api: ApiOpts::default(),
            fmt: ItemFormatOpts::default(),
            package: Some("wasmer/test-py-worker".to_string()),
            use_local_manifest: false,
            new_package_name: None,
        };
        cmd.run_async().await.unwrap();

        let app = std::fs::read_to_string(dir.path().join("app.yaml")).unwrap();
        assert_eq!(
            app,
            r#"kind: wasmer.io/App.v0
name: test-py-worker
owner: wasmer
package: wasmer/test-py-worker
cli_args:
- /src/main.py
debug: false
"#,
        );
    }
}
