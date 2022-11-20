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
#[derive(Debug)]
pub struct HttpRequest {
    pub url: String,
    pub method: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    pub options: HttpRequestOptions,
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

pub trait HttpClient: std::fmt::Debug {
    // TODO: use custom error type!
    fn request(&self, request: HttpRequest) -> BoxFuture<Result<HttpResponse, anyhow::Error>>;
}

pub type DynHttpClient = Arc<dyn HttpClient + Send + Sync + 'static>;
