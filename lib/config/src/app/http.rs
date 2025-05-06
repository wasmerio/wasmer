use super::pretty_duration::PrettyDuration;

/// Defines an HTTP request.
#[derive(
    schemars::JsonSchema, serde::Serialize, serde::Deserialize, PartialEq, Eq, Clone, Debug, Default
)]
pub struct HttpRequest {
    /// Request path.
    pub path: String,

    /// HTTP method.
    ///
    /// Defaults to GET.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,

    /// HTTP headers added to the request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<Vec<HttpHeader>>,

    /// Request body as a string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,

    /// Request timeout.
    ///
    /// Format: 1s, 5m, 11h, ...
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<PrettyDuration>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub expect: Option<HttpRequestExpect>,
}

/// Definition for an HTTP header.
#[derive(
    schemars::JsonSchema, serde::Serialize, serde::Deserialize, PartialEq, Eq, Clone, Debug,
)]
pub struct HttpHeader {
    pub name: String,
    pub value: String,
}

/// Validation checks for an [`HttpRequest`].
#[derive(
    schemars::JsonSchema, serde::Serialize, serde::Deserialize, PartialEq, Eq, Clone, Debug,
)]
pub struct HttpRequestExpect {
    /// Expected HTTP status codes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_codes: Option<Vec<u16>>,

    /// Text that must be present in the response body.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_includes: Option<String>,

    /// Regular expression that must match against the response body.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_regex: Option<String>,
}


#[derive(Debug, Default)]
pub struct HttpRequestBuilder {
    path: String,
    method: Option<String>,
    headers: Option<Vec<HttpHeader>>,
    body: Option<String>,
    timeout: Option<PrettyDuration>,
    expect: Option<HttpRequestExpect>,
}

impl HttpRequestBuilder {
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            ..Default::default()
        }
    }

    pub fn method(mut self, method: impl Into<String>) -> Self {
        self.method = Some(method.into());
        self
    }

    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        let header = HttpHeader {
            name: name.into(),
            value: value.into(),
        };
        self.headers.get_or_insert_with(Vec::new).push(header);
        self
    }

    pub fn body(mut self, body: impl Into<String>) -> Self {
        self.body = Some(body.into());
        self
    }

    pub fn timeout(mut self, timeout: PrettyDuration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub fn expect(mut self, expect: HttpRequestExpect) -> Self {
        self.expect = Some(expect);
        self
    }

    pub fn build(self) -> HttpRequest {
        HttpRequest {
            path: self.path,
            method: self.method,
            headers: self.headers,
            body: self.body,
            timeout: self.timeout,
            expect: self.expect,
        }
    }
}
