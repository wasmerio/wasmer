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
    /// Path to the health check endpoint.
    pub path: String,
    /// HTTP method to use for the health check.
    ///
    /// Defaults to GET.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    /// Interval for the health check.
    ///
    /// Format: 1s, 5m, 11h, ...
    ///
    /// Defaults to 60s.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interval: Option<String>,
    /// Timeout for the health check.
    ///
    /// Deaults to 120s.
    ///
    /// Format: 1s, 5m, 11h, ...
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<String>,
    /// Number of retries before the health check is considered unhealthy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unhealthy_threshold: Option<u32>,
    /// Number of retries before the health check is considered healthy again.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub healthy_threshold: Option<u32>,
    /// Expected status codes that are considered a pass for the health check.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub expected_status_codes: Vec<u16>,
    /// Optional text that is in the body of the response that is considered a pass for the health check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_body_includes: Option<String>,
    /// Regular expression tested against the body that is considered a pass for the health check
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_body_regex: Option<String>,
}
