//! Create a new Edge app.


use anyhow::{bail, Context};
use clap::Parser;
use colored::Colorize;
use dialoguer::Confirm;
use indicatif::ProgressBar;
use is_terminal::IsTerminal;
use std::{path::PathBuf, str::FromStr, time::Duration};
use wasmer_api::{
    query::current_user_with_namespaces,
    types::{DeployAppVersion, Package, UserWithNamespaces},
    WasmerClient,
};
use wasmer_config::{
    app::AppConfigV1,
    package::{Manifest, NamedPackageIdent, PackageIdent, PackageSource, MANIFEST_FILE_NAME},
};
use wasmer_registry::wasmer_env::WasmerEnv;

const TICK: Duration = Duration::from_millis(250);

use crate::{
    commands::{
        app::deploy::{deploy_app_verbose, DeployAppOpts, WaitMode},
        AsyncCliCommand, Login,
    },
    opts::{ApiOpts, ItemFormatOpts},
    utils::{
        package_wizard::{CreateMode, PackageType, PackageWizard},
        prompts::prompt_for_namespace,
    },
};

/// Create a new Edge app.
#[derive(clap::Parser, Debug)]
pub struct CmdAppCreate {
    #[clap(name = "type", short = 't', long)]
    pub template: Option<AppType>,

    #[clap(long)]
    pub publish_package: bool,

    /// Skip local schema validation.
    #[clap(long)]
    pub no_validate: bool,

    /// Do not prompt for user input.
    #[clap(long)]
    pub non_interactive: bool,

    /// Do not interact with any APIs.
    #[clap(long)]
    pub offline: bool,

    /// The owner of the app.
    #[clap(long)]
    pub owner: Option<String>,

    /// The name of the app (can be changed later)
    #[clap(long)]
    pub name: Option<String>,

    /// The path to a YAML file the app config.
    #[clap(long)]
    pub path: Option<PathBuf>,

    /// Do not wait for the app to become reachable.
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
    app_name: Option<String>,
    type_: AppType,
    interactive: bool,
    dir: PathBuf,
    owner: String,
    api: Option<WasmerClient>,
    user: Option<UserWithNamespaces>,
    local_package: Option<(PathBuf, wasmer_config::package::Manifest)>,
}

struct AppCreatorOutput {
    app: AppConfigV1,
    api_pkg: Option<Package>,
    local_package: Option<(PathBuf, wasmer_config::package::Manifest)>,
}

impl AppCreator {
    async fn build_browser_shell_app(self) -> Result<AppCreatorOutput, anyhow::Error> {
        todo!()
        // const WASM_BROWSER_CONTAINER_PACKAGE: &str = "wasmer/wasmer-sh";
        // const WASM_BROWSER_CONTAINER_VERSION: &str = "0.2";

        // eprintln!("A browser web shell wraps another package and runs it in the browser");
        // eprintln!("Select the package to wrap.");

        // let (inner_pkg, _inner_pkg_api) = crate::utils::prompt_for_package(
        //     "Package",
        //     None,
        //     Some(crate::utils::PackageCheckMode::MustExist),
        //     self.api.as_ref(),
        // )
        // .await?;

        // eprintln!("What should be the name of the wrapper package?");

        // let default_name = format!("{}-webshell", inner_pkg.name);
        // let outer_pkg_name =
        //     crate::utils::prompts::prompt_for_ident("Package name", Some(&default_name))?;
        // let outer_pkg_full_name = format!("{}/{}", self.owner, outer_pkg_name);

        // eprintln!("What should be the name of the app?");

        // let default_name = if outer_pkg_name.ends_with("webshell") {
        //     format!("{}-{}", self.owner, outer_pkg_name)
        // } else {
        //     format!("{}-{}-webshell", self.owner, outer_pkg_name)
        // };
        // let app_name = crate::utils::prompts::prompt_for_ident("App name", Some(&default_name))?;

        // // Build the package.

        // let public_dir = self.dir.join("public");
        // if !public_dir.exists() {
        //     std::fs::create_dir_all(&public_dir)?;
        // }

        // let init = serde_json::json!({
        //     "init": format!("{}/{}", inner_pkg.namespace.unwrap(), inner_pkg.name),
        //     "prompt": inner_pkg.name,
        //     "no_welcome": true,
        //     "connect": format!("wss://{app_name}.wasmer.app/.well-known/edge-vpn"),
        // });
        // let init_path = public_dir.join("init.json");
        // std::fs::write(&init_path, init.to_string())
        //     .with_context(|| format!("Failed to write to '{}'", init_path.display()))?;

        // let package = wasmer_config::package::PackageBuilder::new(
        //     outer_pkg_full_name,
        //     "0.1.0".parse().unwrap(),
        //     format!("{} web shell", inner_pkg.name),
        // )
        // .rename_commands_to_raw_command_name(false)
        // .build()?;

        // let manifest = wasmer_config::package::ManifestBuilder::new(package)
        //     .with_dependency(
        //         WASM_BROWSER_CONTAINER_PACKAGE,
        //         WASM_BROWSER_CONTAINER_VERSION.to_string().parse().unwrap(),
        //     )
        //     .map_fs("public", PathBuf::from("public"))
        //     .build()?;

        // let manifest_path = self.dir.join("wasmer.toml");

        // let raw = manifest.to_string()?;
        // eprintln!(
        //     "Writing wasmer.toml package to '{}'",
        //     manifest_path.display()
        // );
        // std::fs::write(&manifest_path, raw)?;

        // let app_cfg = AppConfigV1 {
        //     app_id: None,
        //     name: app_name,
        //     owner: Some(self.owner.clone()),
        //     cli_args: None,
        //     env: Default::default(),
        //     volumes: None,
        //     domains: None,
        //     scaling: None,
        //     package: NamedPackageIdent {
        //         registry: None,
        //         namespace: Some(self.owner),
        //         name: outer_pkg_name,
        //         tag: None,
        //     }
        //     .into(),
        //     capabilities: None,
        //     scheduled_tasks: None,
        //     debug: Some(false),
        //     extra: Default::default(),
        //     health_checks: None,
        // };

        // Ok(AppCreatorOutput {
        //     app: app_cfg,
        //     api_pkg: None,
        //     local_package: Some((self.dir, manifest)),
        // })
    }

    async fn build_app(self) -> Result<AppCreatorOutput, anyhow::Error> {
        todo!()
        // let package_opt: Option<PackageIdent> = if let Some(package) = self.package {
        //     Some(package.parse()?)
        // } else if let Some((_, local)) = self.local_package.as_ref() {
        //     let full = format!(
        //         "{}@{}",
        //         local.package.clone().unwrap().name,
        //         local.package.clone().unwrap().version
        //     );
        //     let mut pkg_ident = NamedPackageIdent::from_str(&local.package.clone().unwrap().name)
        //         .with_context(|| {
        //         format!("local package manifest has invalid name: '{full}'")
        //     })?;
        //     // pkg
        //     // Pin the version.
        //     pkg_ident.tag = Some(wasmer_config::package::Tag::VersionReq(
        //         local.package.clone().unwrap().version.,
        //     ));

        //     if self.interactive {
        //         eprintln!("Found local package: '{}'", full.green());

        //         let msg = format!("Use package '{pkg_ident}'");

        //         let should_use = Confirm::new()
        //             .with_prompt(&msg)
        //             .interact_opt()?
        //             .unwrap_or_default();

        //         if should_use {
        //             Some(pkg_ident)
        //         } else {
        //             None
        //         }
        //     } else {
        //         Some(pkg_ident)
        //     }
        // } else {
        //     None
        // };

        // let (pkg, api_pkg, local_package) = if let Some(pkg) = package_opt {
        //     if let Some(api) = &self.api {
        //         let p2 =
        //             wasmer_api::query::get_package(api, format!("{}/{}", pkg.namespace, pkg.name))
        //                 .await?;

        //         (pkg.into(), p2, self.local_package)
        //     } else {
        //         (pkg.into(), None, self.local_package)
        //     }
        // } else {
        //     eprintln!("No package found or specified.");

        //     let ty = match self.type_ {
        //         AppType::HttpServer => None,
        //         AppType::StaticWebsite => Some(PackageType::StaticWebsite),
        //         AppType::BrowserShell => None,
        //         AppType::JsWorker => Some(PackageType::JsWorker),
        //         AppType::PyApplication => Some(PackageType::PyApplication),
        //     };

        //     let create_mode = match ty {
        //         Some(PackageType::StaticWebsite)
        //         | Some(PackageType::JsWorker)
        //         | Some(PackageType::PyApplication) => CreateMode::Create,
        //         // Only static website creation is currently supported.
        //         _ => CreateMode::SelectExisting,
        //     };

        //     let w = PackageWizard {
        //         path: self.dir.clone(),
        //         name: self.new_package_name.clone(),
        //         type_: ty,
        //         create_mode,
        //         namespace: Some(self.owner.clone()),
        //         namespace_default: self.user.as_ref().map(|u| u.username.clone()),
        //         user: self.user.clone(),
        //     };

        //     let output = w.run(self.api.as_ref()).await?;
        //     (
        //         output.ident,
        //         output.api,
        //         output
        //             .local_path
        //             .and_then(move |x| Some((x, output.local_manifest?))),
        //     )
        // };

        // let ident = pkg.as_ident().context("unnamed packages not supported")?;

        // let name = if let Some(name) = self.app_name {
        //     name
        // } else {
        //     let default = match self.type_ {
        //         AppType::HttpServer | AppType::StaticWebsite => {
        //             format!("{}-{}", ident.namespace, ident.name)
        //         }
        //         AppType::JsWorker | AppType::PyApplication => {
        //             format!("{}-{}-worker", ident.namespace, ident.name)
        //         }
        //         AppType::BrowserShell => {
        //             format!("{}-{}-webshell", ident.namespace, ident.name)
        //         }
        //     };

        //     dialoguer::Input::new()
        //         .with_prompt("What should be the name of the app? <NAME>.wasmer.app")
        //         .with_initial_text(default)
        //         .interact_text()
        //         .unwrap()
        // };

        // let cli_args = match self.type_ {
        //     AppType::PyApplication => Some(vec!["/src/main.py".to_string()]),
        //     AppType::JsWorker => Some(vec!["/src/index.js".to_string()]),
        //     _ => None,
        // };

        // // TODO: check if name already exists.
        // let cfg = AppConfigV1 {
        //     app_id: None,
        //     owner: Some(self.owner.clone()),
        //     volumes: None,
        //     name,
        //     env: Default::default(),
        //     scaling: None,
        //     // CLI args are only set for JS and Py workers for now.
        //     cli_args,
        //     // TODO: allow setting the description.
        //     // description: Some("".to_string()),
        //     package: pkg.clone(),
        //     capabilities: None,
        //     scheduled_tasks: None,
        //     debug: Some(false),
        //     domains: None,
        //     extra: Default::default(),
        //     health_checks: None,
        // };

        // Ok(AppCreatorOutput {
        //     app: cfg,
        //     api_pkg,
        //     local_package,
        // })
    }
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppCreate {
    type Output = (AppConfigV1, Option<DeployAppVersion>);

    async fn run_async(self) -> Result<(AppConfigV1, Option<DeployAppVersion>), anyhow::Error> {
        todo!()
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
            publish_package: false,
            no_validate: false,
            non_interactive: true,
            offline: true,
            owner: Some("testuser".to_string()),
            name: Some("static-site-1".to_string()),
            path: Some(dir.path().to_owned()),
            no_wait: true,
            api: ApiOpts::default(),
            fmt: ItemFormatOpts::default(),
            package: Some("testuser/static-site1@0.1.0".to_string()),
        };
        cmd.run_async().await.unwrap();

        let app = std::fs::read_to_string(dir.path().join("app.yaml")).unwrap();
        assert_eq!(
            app,
            r#"---
kind: wasmer.io/App.v0
name: static-site-1
owner: testuser
package: testuser/static-site-1@0.1.0
debug: false
"#,
        );
    }

    #[tokio::test]
    async fn test_app_create_offline_with_package() {
        let dir = tempfile::tempdir().unwrap();

        let cmd = CmdAppCreate {
            template: Some(AppType::HttpServer),
            publish_package: false,
            no_validate: false,
            non_interactive: true,
            offline: true,
            owner: Some("wasmer".to_string()),
            name: Some("testapp".to_string()),
            path: Some(dir.path().to_owned()),
            no_wait: true,
            api: ApiOpts::default(),
            fmt: ItemFormatOpts::default(),
            package: Some("wasmer/testpkg".to_string()),
        };
        cmd.run_async().await.unwrap();

        let app = std::fs::read_to_string(dir.path().join("app.yaml")).unwrap();
        assert_eq!(
            app,
            r#"---
kind: wasmer.io/App.v0
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
            publish_package: false,
            no_validate: false,
            non_interactive: true,
            offline: true,
            owner: Some("wasmer".to_string()),
            name: Some("test-js-worker".to_string()),
            path: Some(dir.path().to_owned()),
            no_wait: true,
            api: ApiOpts::default(),
            fmt: ItemFormatOpts::default(),
            package: Some("wasmer/test-js-worker".to_string()),
        };
        cmd.run_async().await.unwrap();

        let app = std::fs::read_to_string(dir.path().join("app.yaml")).unwrap();
        assert_eq!(
            app,
            r#"---
kind: wasmer.io/App.v0
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
            publish_package: false,
            no_validate: false,
            non_interactive: true,
            offline: true,
            owner: Some("wasmer".to_string()),
            name: Some("test-py-worker".to_string()),
            path: Some(dir.path().to_owned()),
            no_wait: true,
            api: ApiOpts::default(),
            fmt: ItemFormatOpts::default(),
            package: Some("wasmer/test-py-worker".to_string()),
        };
        cmd.run_async().await.unwrap();

        let app = std::fs::read_to_string(dir.path().join("app.yaml")).unwrap();
        assert_eq!(
            app,
            r#"---
kind: wasmer.io/App.v0
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
