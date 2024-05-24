use std::{env, time::Duration};

use anyhow::Context;
use futures::{future::BoxFuture, TryStreamExt};
use std::convert::TryFrom;
use tokio::runtime::Handle;

use super::{HttpRequest, HttpResponse};

#[derive(Clone, Debug)]
pub struct ReqwestHttpClient {
    handle: Handle,
    connect_timeout: Duration,
    response_body_chunk_timeout: Option<std::time::Duration>,
}

impl Default for ReqwestHttpClient {
    fn default() -> Self {
        Self {
            handle: Handle::current(),
            connect_timeout: Self::DEFAULT_CONNECT_TIMEOUT,
            response_body_chunk_timeout: None,
        }
    }
}

impl ReqwestHttpClient {
    const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

    pub fn with_connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout;
        self
    }

    pub fn with_response_body_chunk_timeout(mut self, timeout: std::time::Duration) -> Self {
        self.response_body_chunk_timeout = Some(timeout);
        self
    }

    async fn request(&self, request: HttpRequest) -> Result<HttpResponse, anyhow::Error> {
        let method = reqwest::Method::try_from(request.method.as_str())
            .with_context(|| format!("Invalid http method {}", request.method))?;

        // TODO: use persistent client?
        let builder = {
            let _guard = Handle::try_current().map_err(|_| self.handle.enter());
            let mut builder = reqwest::ClientBuilder::new().connect_timeout(self.connect_timeout);
            if let Some(proxy) = get_proxy()? {
                builder = builder.proxy(proxy);
            }
            builder
        };
        let client = builder.build().context("failed to create reqwest client")?;

        let mut builder = client.request(method, request.url.as_str());
        for (header, val) in &request.headers {
            builder = builder.header(header, val);
        }

        if let Some(body) = request.body {
            builder = builder.body(reqwest::Body::from(body));
        }

        let request = builder
            .build()
            .context("Failed to construct http request")?;

        let mut response = client.execute(request).await?;
        let headers = std::mem::take(response.headers_mut());

        let status = response.status();
        let data = if let Some(timeout) = self.response_body_chunk_timeout {
            let mut stream = response.bytes_stream();
            let mut buf = Vec::new();
            while let Some(chunk) = tokio::time::timeout(timeout, stream.try_next()).await?? {
                buf.extend_from_slice(&chunk);
            }
            buf
        } else {
            response.bytes().await?.to_vec()
        };

        Ok(HttpResponse {
            status,
            redirected: false,
            body: Some(data),
            headers,
        })
    }
}

impl super::HttpClient for ReqwestHttpClient {
    fn request(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, anyhow::Error>> {
        let client = self.clone();
        let f = async move { client.request(request).await };
        Box::pin(f)
    }
}

pub fn get_proxy() -> Result<Option<reqwest::Proxy>, anyhow::Error> {
    if let Ok(scheme) = env::var("http_proxy").or_else(|_| env::var("HTTP_PROXY")) {
        let proxy = reqwest::Proxy::all(scheme)?;

        Ok(Some(proxy))
    } else {
        Ok(None)
    }
}
