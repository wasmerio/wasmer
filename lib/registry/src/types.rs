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
