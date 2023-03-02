use anyhow::Context;

use crate::RegistryClient;

use crate::graphql::mutations;
use crate::types::{PublishDeployAppOutput, PublishDeployAppRawVars};

/// Generate a Deploy token for for the given Deploy app version id.
pub async fn generate_deploy_token(
    client: &RegistryClient,
    app_version_id: String,
) -> Result<String, anyhow::Error> {
    let vars = mutations::generate_deploy_token::Variables { app_version_id };
    let res = client
        .execute::<mutations::GenerateDeployToken>(vars)
        .await?;
    let token = res
        .generate_deploy_token
        .context("Query did not return a token")?
        .token;

    Ok(token)
}

/// Publish a Deploy app.
///
/// Takes a raw, unvalidated deployment config.
// TODO: Add a variant of this query that takes a typed DeployV1 config.
pub async fn publish_deploy_app_raw(
    client: &RegistryClient,
    data: PublishDeployAppRawVars,
) -> Result<PublishDeployAppOutput, anyhow::Error> {
    let vars2 = mutations::publish_deploy_app::Variables {
        name: data.name,
        owner: data.namespace,
        config: serde_json::to_string(&data.config)?,
    };

    let version = client
        .execute::<mutations::PublishDeployApp>(vars2)
        .await?
        .publish_deploy_app
        .context("Query did not return data")?
        .deploy_app_version;
    let app = version.app.context("Query did not return expected data")?;

    Ok(PublishDeployAppOutput {
        app_id: app.id,
        app_name: app.name,
        version_id: version.id,
        version_name: version.version,
        owner_name: app.owner.global_name,
    })
}
