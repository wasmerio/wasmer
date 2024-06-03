/// Defines an HTTP request.
#[derive(
    schemars::JsonSchema, serde::Serialize, serde::Deserialize, PartialEq, Eq, Clone, Debug,
)]
pub struct HttpRequest {
    /// Request path.
    pub path: String,

    /// HTTP method.
    ///
    /// Defaults to GET.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,

    /// Request body as a string.
    pub body: Option<String>,

    /// Request timeout.
    ///
    /// Format: 1s, 5m, 11h, ...
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<String>,

    pub expect: Option<HttpRequestExpect>,
}

/// Validation checks for an [`HttpRequest`].
#[derive(
    schemars::JsonSchema, serde::Serialize, serde::Deserialize, PartialEq, Eq, Clone, Debug,
)]
pub struct HttpRequestExpect {
    /// Expected HTTP status codes.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub status_codes: Vec<u16>,

    /// Text that must be present in the response body.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_includes: Option<String>,

    /// Regular expression that must match against the response body.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_regex: Option<String>,
}
