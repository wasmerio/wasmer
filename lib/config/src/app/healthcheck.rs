#[derive(
    schemars::JsonSchema, serde::Serialize, serde::Deserialize, PartialEq, Eq, Clone, Debug,
)]
pub enum HealthCheckV1 {
    #[serde(rename = "http")]
    Http(HealthCheckHttpV1),
}

/// Health check configuration for http endpoints.
#[derive(
    schemars::JsonSchema, serde::Serialize, serde::Deserialize, PartialEq, Eq, Clone, Debug,
)]
pub struct HealthCheckHttpV1 {
    #[serde(flatten)]
    pub request: super::HttpRequest,

    /// Interval for the health check.
    ///
    /// Format: 1s, 5m, 11h, ...
    ///
    /// Defaults to 60s.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interval: Option<String>,

    /// Number of retries before the health check is considered unhealthy.
    ///
    /// Defaults to 1.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unhealthy_threshold: Option<u32>,

    /// Number of retries before the health check is considered healthy again.
    ///
    /// Defaults to 1.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub healthy_threshold: Option<u32>,
}
