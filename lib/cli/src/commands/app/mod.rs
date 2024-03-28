//! Edge app commands.

pub mod create;
pub mod delete;
pub mod get;
pub mod info;
pub mod list;
pub mod logs;
pub mod version;

mod util;

use std::{io::Write, time::Duration};

use anyhow::{bail, Context};
use edge_schema::schema::AppConfigV1;
use wasmer_api::{
    types::{DeployApp, DeployAppVersion},
    WasmerClient,
};

use crate::commands::AsyncCliCommand;

/// Manage Wasmer Deploy apps.
#[derive(clap::Subcommand, Debug)]
pub enum CmdApp {
    Get(get::CmdAppGet),
    Info(info::CmdAppInfo),
    List(list::CmdAppList),
    Logs(logs::CmdAppLogs),
    Create(create::CmdAppCreate),
    Delete(delete::CmdAppDelete),
    #[clap(subcommand)]
    Version(version::CmdAppVersion),
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdApp {
    type Output = ();

    async fn run_async(self) -> Result<(), anyhow::Error> {
        match self {
            Self::Get(cmd) => {
                cmd.run_async().await?;
                Ok(())
            }
            Self::Info(cmd) => {
                cmd.run_async().await?;
                Ok(())
            }
            Self::Create(cmd) => {
                cmd.run_async().await?;
                Ok(())
            }
            Self::List(cmd) => cmd.run_async().await,
            Self::Logs(cmd) => cmd.run_async().await,
            Self::Delete(cmd) => cmd.run_async().await,
            Self::Version(cmd) => cmd.run_async().await,
        }
    }
}

pub struct DeployAppOpts<'a> {
    pub app: &'a AppConfigV1,
    // Original raw yaml config.
    // Present here to enable forwarding unknown fields to the backend, which
    // preserves forwards-compatibility for schema changes.
    pub original_config: Option<serde_yaml::Value>,
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
        format!("{}/{}", owner, app.name)
    } else {
        app.name.clone()
    };

    let make_default = opts.make_default;

    eprintln!("Deploying app {pretty_name}...\n");

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

    let full_name = format!("{}/{}", app.owner.global_name, app.name);

    eprintln!(" ✅ App {full_name} was successfully deployed!");
    eprintln!();
    eprintln!("> App URL: {}", app.url);
    eprintln!("> Versioned URL: {}", version.url);
    eprintln!("> Admin dashboard: {}", app.admin_url);

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
                    bail!("\nApp still not reachable after 5 minutes...");
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
                            eprintln!("Deployment complete");
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
