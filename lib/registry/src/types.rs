use serde::Deserialize;

/// Payload for publishing a new Deploy app.
#[derive(Clone, Debug)]
pub struct PublishDeployAppRawVars {
    /// Name for the app.
    /// If not specified a random name will be generated.
    pub name: Option<String>,

    /// The namespace to deploy the app to.
    ///
    /// If not specified the app will be deployed under the current users
    /// namespace.
    pub namespace: Option<String>,

    /// The raw DeployV1 configuration.
    pub config: serde_json::Value,
}

/// Data for a deployed app.
#[derive(Clone, Debug)]
pub struct PublishDeployAppOutput {
    pub app_id: String,
    pub app_name: String,
    pub version_id: String,
    pub version_name: String,
    pub owner_name: String,
}

#[derive(Clone, Debug)]
pub struct NewNonceOutput {
    pub auth_url: String,
}

/// Payload from the frontend after the user has authenticated.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TokenStatus {
    Cancelled,
    Authorized,
}

fn token_status_deserializer<'de, D>(deserializer: D) -> Result<TokenStatus, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    match s.as_str() {
        "cancelled" => Ok(TokenStatus::Cancelled),
        "authorized" => Ok(TokenStatus::Authorized),
        _ => Err(serde::de::Error::custom(format!(
            "invalid token status: {}",
            s
        ))),
    }
}

/// Payload from the frontend after the user has authenticated.
///
/// This has the token that we need to set in the WASMER_TOML file.
#[derive(Clone, Debug, Deserialize)]
pub struct ValidatedNonceOutput {
    pub token: String,
    #[serde(deserialize_with = "token_status_deserializer")]
    pub status: TokenStatus,
}
