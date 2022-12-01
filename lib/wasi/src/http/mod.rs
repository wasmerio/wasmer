#[cfg(feature = "host-reqwest")]
pub mod reqwest;

use std::sync::Arc;

use futures::future::BoxFuture;

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
            .field(
                "body",
                &self.body.as_deref().map(|b| String::from_utf8_lossy(b)),
            )
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
            .field(
                "body",
                &self.body.as_deref().map(|b| String::from_utf8_lossy(b)),
            )
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
