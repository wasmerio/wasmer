use std::{collections::HashSet, sync::Arc};

use futures::future::BoxFuture;

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
    pub url: String,
    pub method: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    pub options: HttpRequestOptions,
}

impl std::fmt::Debug for HttpRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpRequest")
            .field("url", &self.url)
            .field("method", &self.method)
            .field("headers", &self.headers)
            .field("body", &self.body.as_deref().map(String::from_utf8_lossy))
            .field("options", &self.options)
            .finish()
    }
}

// TODO: use types from http crate?
pub struct HttpResponse {
    pub pos: usize,
    pub body: Option<Vec<u8>>,
    pub ok: bool,
    pub redirected: bool,
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
}

impl std::fmt::Debug for HttpResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpResponse")
            .field("pos", &self.pos)
            .field("body", &self.body.as_deref().map(String::from_utf8_lossy))
            .field("ok", &self.ok)
            .field("redirected", &self.redirected)
            .field("status", &self.status)
            .field("status_text", &self.status_text)
            .field("headers", &self.headers)
            .finish()
    }
}

pub trait HttpClient: std::fmt::Debug {
    // TODO: use custom error type!
    fn request(&self, request: HttpRequest) -> BoxFuture<Result<HttpResponse, anyhow::Error>>;
}

pub type DynHttpClient = Arc<dyn HttpClient + Send + Sync + 'static>;
