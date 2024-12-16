pub(crate) mod render;
use anyhow::Context;
use colored::Colorize;
use std::{
    env::current_dir,
    path::{Path, PathBuf},
    str::FromStr,
};
use wasmer_backend_api::{types::Secret as BackendSecret, WasmerClient};

use crate::commands::app::util::{get_app_config_from_dir, prompt_app_ident, AppIdent};

#[derive(serde::Serialize, serde::Deserialize)]
pub(super) struct Secret {
    pub name: String,
    pub value: String,
}

pub(super) async fn read_secrets_from_file(path: &Path) -> anyhow::Result<Vec<Secret>> {
    let mut ret = vec![];
    for item in dotenvy::from_path_iter(path)? {
        let (name, value) = item?;
        ret.push(Secret { name, value })
    }
    Ok(ret)
}

pub(super) async fn get_secret_by_name(
    client: &WasmerClient,
    app_id: &str,
    secret_name: &str,
) -> anyhow::Result<Option<BackendSecret>> {
    wasmer_backend_api::query::get_app_secret_by_name(client, app_id, secret_name).await
}
pub(crate) async fn get_secrets(
    client: &WasmerClient,
    app_id: &str,
) -> anyhow::Result<Vec<wasmer_backend_api::types::Secret>> {
    wasmer_backend_api::query::get_all_app_secrets(client, app_id).await
}

pub(crate) async fn get_secret_value(
    client: &WasmerClient,
    secret: &wasmer_backend_api::types::Secret,
) -> anyhow::Result<String> {
    wasmer_backend_api::query::get_app_secret_value_by_id(client, secret.id.clone().into_inner())
        .await?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "No value found for secret with name '{}'",
                secret.name.bold()
            )
        })
}

pub(crate) async fn get_secret_value_by_name(
    client: &WasmerClient,
    app_id: &str,
    secret_name: &str,
) -> anyhow::Result<String> {
    match get_secret_by_name(client, app_id, secret_name).await? {
        Some(secret) => get_secret_value(client, &secret).await,
        None => anyhow::bail!("No secret found with name {secret_name} for app {app_id}"),
    }
}

pub(crate) async fn reveal_secrets(
    client: &WasmerClient,
    app_id: &str,
) -> anyhow::Result<Vec<Secret>> {
    let secrets = wasmer_backend_api::query::get_all_app_secrets(client, app_id).await?;
    let mut ret = vec![];
    for secret in secrets {
        let name = secret.name.clone();
        let value = get_secret_value(client, &secret).await?;
        ret.push(Secret { name, value });
    }

    Ok(ret)
}

/// Utility struct used just to implement [`CliRender`].
#[derive(Debug, serde::Serialize)]
pub(super) struct BackendSecretWrapper(pub BackendSecret);

impl From<BackendSecret> for BackendSecretWrapper {
    fn from(value: BackendSecret) -> Self {
        Self(value)
    }
}

/// A secrets-specific app to retrieve an app identifier.
pub(super) async fn get_app_id(
    client: &WasmerClient,
    app: Option<&AppIdent>,
    app_dir_path: Option<&PathBuf>,
    quiet: bool,
    non_interactive: bool,
) -> anyhow::Result<String> {
    if let Some(app_id) = app {
        let app = app_id.resolve(client).await?;
        return Ok(app.id.into_inner());
    }

    let path = if let Some(path) = app_dir_path {
        path.clone()
    } else {
        current_dir()?
    };

    if let Ok(r) = get_app_config_from_dir(&path) {
        let (app, _) = r;

        let app_name = if let Some(owner) = &app.owner {
            format!(
                "{owner}/{}",
                &app.name.clone().context("App name has to be specified")?
            )
        } else {
            app.name
                .clone()
                .context("App name has to be specified")?
                .to_string()
        };

        let id = if let Some(id) = &app.app_id {
            Some(id.clone())
        } else if let Ok(app_ident) = AppIdent::from_str(&app_name) {
            if let Ok(app) = app_ident.resolve(client).await {
                Some(app.id.into_inner())
            } else {
                if !quiet {
                    eprintln!("{}: the app found in {} does not exist.\n{}: maybe it was not deployed yet?", 
                            "Warning".bold().yellow(), 
                            format!("'{}'", path.display()).dimmed(), 
                            "Hint".bold());
                }
                None
            }
        } else {
            None
        };

        if let Some(id) = id {
            if !quiet {
                if let Some(owner) = &app.owner {
                    eprintln!(
                        "Managing secrets related to app {} ({owner}).",
                        app.name.context("App name has to be specified")?.bold()
                    );
                } else {
                    eprintln!(
                        "Managing secrets related to app {}.",
                        app.name.context("App name has to be specified")?.bold()
                    );
                }
            }
            return Ok(id);
        }
    } else if let Some(path) = app_dir_path {
        anyhow::bail!(
            "No app configuration file found in path {}.",
            path.display()
        )
    }

    if non_interactive {
        anyhow::bail!("No app id given. Provide one using the `--app` flag.")
    } else {
        let id = prompt_app_ident("Enter the name of the app")?;
        let app = id.resolve(client).await?;
        Ok(app.id.into_inner())
    }
}
