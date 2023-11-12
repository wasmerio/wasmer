use std::{collections::HashSet, ops::Deref, sync::Arc};

use futures::future::BoxFuture;
use http::{HeaderMap, Method, StatusCode};
use url::Url;

/// Defines http client permissions.
#[derive(Clone, Debug)]
pub struct HttpClientCapabilityV1 {
    pub allow_all: bool,
    pub allowed_hosts: HashSet<String>,
}

impl HttpClientCapabilityV1 {
    pub fn new() -> Self {
        Self {
            allow_all: false,
            allowed_hosts: HashSet::new(),
        }
    }

    pub fn new_allow_all() -> Self {
        Self {
            allow_all: true,
            allowed_hosts: HashSet::new(),
        }
    }

    pub fn is_deny_all(&self) -> bool {
        !self.allow_all && self.allowed_hosts.is_empty()
    }

    pub fn can_access_domain(&self, domain: &str) -> bool {
        self.allow_all || self.allowed_hosts.contains(domain)
    }

    pub fn update(&mut self, other: HttpClientCapabilityV1) {
        let HttpClientCapabilityV1 {
            allow_all,
            allowed_hosts,
        } = other;
        self.allow_all |= allow_all;
        self.allowed_hosts.extend(allowed_hosts);
    }
}

impl Default for HttpClientCapabilityV1 {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Default)]
pub struct HttpRequestOptions {
    pub gzip: bool,
    pub cors_proxy: Option<String>,
}

// TODO: use types from http crate?
pub struct HttpRequest {
    pub url: Url,
    pub method: Method,
    pub headers: HeaderMap,
    pub body: Option<Vec<u8>>,
    pub options: HttpRequestOptions,
}

impl HttpRequest {
    fn from_http_parts(parts: http::request::Parts, body: impl Into<Option<Vec<u8>>>) -> Self {
        let http::request::Parts {
            method,
            uri,
            headers,
            ..
        } = parts;

        HttpRequest {
            url: uri.to_string().parse().unwrap(),
            method,
            headers,
            body: body.into(),
            options: HttpRequestOptions::default(),
        }
    }
}

impl std::fmt::Debug for HttpRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let HttpRequest {
            url,
            method,
            headers,
            body,
            options,
        } = self;

        f.debug_struct("HttpRequest")
            .field("url", &format_args!("{}", url))
            .field("method", method)
            .field("headers", headers)
            .field("body", &body.as_deref().map(String::from_utf8_lossy))
            .field("options", &options)
            .finish()
    }
}

impl From<http::Request<Option<Vec<u8>>>> for HttpRequest {
    fn from(value: http::Request<Option<Vec<u8>>>) -> Self {
        let (parts, body) = value.into_parts();
        HttpRequest::from_http_parts(parts, body)
    }
}

impl From<http::Request<Vec<u8>>> for HttpRequest {
    fn from(value: http::Request<Vec<u8>>) -> Self {
        let (parts, body) = value.into_parts();
        HttpRequest::from_http_parts(parts, body)
    }
}

impl From<http::Request<&str>> for HttpRequest {
    fn from(value: http::Request<&str>) -> Self {
        value.map(|body| body.to_string()).into()
    }
}

impl From<http::Request<String>> for HttpRequest {
    fn from(value: http::Request<String>) -> Self {
        let (parts, body) = value.into_parts();
        HttpRequest::from_http_parts(parts, body.into_bytes())
    }
}

impl From<http::Request<()>> for HttpRequest {
    fn from(value: http::Request<()>) -> Self {
        let (parts, _) = value.into_parts();
        HttpRequest::from_http_parts(parts, None)
    }
}

// TODO: use types from http crate?
pub struct HttpResponse {
    pub body: Option<Vec<u8>>,
    pub redirected: bool,
    pub status: StatusCode,
    pub headers: HeaderMap,
}

impl HttpResponse {
    pub fn is_ok(&self) -> bool {
        !self.status.is_client_error() && !self.status.is_server_error()
    }
}

impl std::fmt::Debug for HttpResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let HttpResponse {
            body,
            redirected,
            status,
            headers,
        } = self;

        f.debug_struct("HttpResponse")
            .field("ok", &self.is_ok())
            .field("redirected", &redirected)
            .field("status", &status)
            .field("headers", &headers)
            .field("body", &body.as_deref().map(String::from_utf8_lossy))
            .finish()
    }
}

pub trait HttpClient: std::fmt::Debug {
    // TODO: use custom error type!
    fn request(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, anyhow::Error>>;
}

impl<D, C> HttpClient for D
where
    D: Deref<Target = C> + std::fmt::Debug,
    C: HttpClient + ?Sized + 'static,
{
    fn request(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, anyhow::Error>> {
        let client = &**self;
        client.request(request)
    }
}

pub type DynHttpClient = Arc<dyn HttpClient + Send + Sync + 'static>;
