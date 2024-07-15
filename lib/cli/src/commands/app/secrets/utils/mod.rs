pub(crate) mod render;

use colored::Colorize;
use std::path::Path;
use wasmer_api::{types::Secret as BackendSecret, WasmerClient};

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
    wasmer_api::query::get_app_secret_by_name(client, app_id, secret_name).await
}
pub(crate) async fn get_secrets(
    client: &WasmerClient,
    app_id: &str,
) -> anyhow::Result<Vec<wasmer_api::types::Secret>> {
    wasmer_api::query::get_all_app_secrets(client, app_id).await
}

pub(crate) async fn get_secret_value(
    client: &WasmerClient,
    secret: &wasmer_api::types::Secret,
) -> anyhow::Result<String> {
    wasmer_api::query::get_app_secret_value_by_id(client, secret.id.clone().into_inner())
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
    let secrets = wasmer_api::query::get_all_app_secrets(client, app_id).await?;
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
