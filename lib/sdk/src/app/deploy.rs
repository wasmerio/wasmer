use std::time::Duration;

use anyhow::Context as _;
use reqwest;
use thiserror::Error;
use wasmer_backend_api::{
    WasmerClient,
    types::{DeployApp, DeployAppVersion, PublishDeployAppVars},
};
use wasmer_config::{app::AppConfigV1, package::PackageSource};

use crate::package::publish::{
    PackagePublishError, PublishOptions, PublishProgress, PublishWait, publish_package_directory,
};

/// When waiting for an app deployment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitMode {
    /// Wait for the deployment to be created.
    Deployed,
    /// Wait until the deployment becomes reachable over the network.
    Reachable,
}

/// Progress events during deployment.
#[derive(Debug, Clone)]
pub enum DeployProgress {
    Publishing(PublishProgress),
    Deploying,
    Waiting(WaitMode),
}

/// Options for deploying an app.
#[derive(Debug, Clone)]
pub struct DeployOptions {
    pub owner: Option<String>,
    pub make_default: bool,
    pub wait: WaitMode,
    pub publish_package: bool,
    pub package_namespace: Option<String>,
    pub publish_timeout: Duration,
}

impl Default for DeployOptions {
    fn default() -> Self {
        Self {
            owner: None,
            make_default: true,
            wait: WaitMode::Reachable,
            publish_package: true,
            package_namespace: None,
            publish_timeout: Duration::from_secs(60 * 5),
        }
    }
}

/// Error that can occur during app deployment.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum DeployError {
    #[error("missing owner in configuration or options")]
    MissingOwner,
    #[error("missing app name in configuration")]
    MissingName,
    #[error("package publish error: {0}")]
    Publish(#[from] PackagePublishError),
    #[error("yaml error: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("backend API error: {0}")]
    Api(#[from] anyhow::Error),
    #[error("app http probe failed: {message}")]
    AppHttpCheck { message: String },
}

/// Deploy an app based on the provided app configuration and options.
pub async fn deploy_app<F>(
    client: &WasmerClient,
    mut config: AppConfigV1,
    opts: DeployOptions,
    mut progress: F,
) -> Result<(DeployApp, DeployAppVersion), DeployError>
where
    F: FnMut(DeployProgress) + Send,
{
    let owner = opts
        .owner
        .clone()
        .or_else(|| config.owner.clone())
        .ok_or(DeployError::MissingOwner)?;

    if opts.publish_package
        && let PackageSource::Path(ref path) = config.package {
            let publish_opts = PublishOptions {
                namespace: Some(
                    opts.package_namespace
                        .clone()
                        .unwrap_or_else(|| owner.clone()),
                ),
                timeout: opts.publish_timeout,
                wait: PublishWait::Container,
                ..Default::default()
            };
            let ident = publish_package_directory(client, path.as_ref(), publish_opts, |e| {
                progress(DeployProgress::Publishing(e));
            })
            .await?;
            config.package = ident.into();
        }

    let name = config.name.clone().ok_or(DeployError::MissingName)?;

    let config_value = config.clone().to_yaml_value()?;
    let raw_config = serde_yaml::to_string(&config_value)?.trim().to_string() + "\n";

    progress(DeployProgress::Deploying);
    let version = wasmer_backend_api::query::publish_deploy_app(
        client,
        PublishDeployAppVars {
            config: raw_config,
            name: name.clone().into(),
            owner: Some(owner.clone().into()),
            make_default: Some(opts.make_default),
        },
    )
    .await?;

    progress(DeployProgress::Waiting(opts.wait));
    wait_app(client, &version, opts.wait, opts.make_default).await
}

async fn wait_app(
    client: &WasmerClient,
    version: &DeployAppVersion,
    wait: WaitMode,
    make_default: bool,
) -> Result<(DeployApp, DeployAppVersion), DeployError> {
    const PROBE_TIMEOUT: Duration = Duration::from_secs(60 * 5);
    use wasmer_config::app::HEADER_APP_VERSION_ID;

    let app_id = version
        .app
        .as_ref()
        .ok_or_else(|| DeployError::Api(anyhow::anyhow!("app field empty")))?
        .id
        .inner()
        .to_string();

    let app = wasmer_backend_api::query::get_app_by_id(client, app_id.clone())
        .await
        .map_err(DeployError::Api)?;

    match wait {
        WaitMode::Deployed => {}
        WaitMode::Reachable => {
            tokio::time::sleep(Duration::from_secs(2)).await;
            let check_url = if make_default { &app.url } else { &version.url };
            let http = reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(90))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .context("failed to build HTTP client")?;
            let start = tokio::time::Instant::now();
            let mut sleep_ms = 1_000u64;
            loop {
                if start.elapsed() > Duration::from_secs(60 * 5) {
                    return Err(DeployError::AppHttpCheck {
                        message: format!(
                            "timed out waiting for app version '{}' to become reachable at '{}' (tried for {} seconds)",
                            version.id.inner(),
                            check_url,
                            PROBE_TIMEOUT.as_secs(),
                        ),
                    });
                }
                if let Ok(res) = http.get(check_url).send().await {
                    let header = res
                        .headers()
                        .get(HEADER_APP_VERSION_ID)
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("");
                    if header == version.id.inner() {
                        break;
                    }
                }
                let to_sleep = Duration::from_millis(sleep_ms);
                tokio::time::sleep(to_sleep).await;
                sleep_ms = (sleep_ms * 2).min(10_000);
            }
        }
    }

    Ok((app, version.clone()))
}
