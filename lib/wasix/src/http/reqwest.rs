use std::time::Duration;

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

    #[tracing::instrument(skip_all, fields(method=?request.method, url=%request.url))]
    async fn request(&self, request: HttpRequest) -> Result<HttpResponse, anyhow::Error> {
        let method = reqwest::Method::try_from(request.method.as_str())
            .with_context(|| format!("Invalid http method {}", request.method))?;

        // TODO: use persistent client?
        let builder = {
            let _guard = Handle::try_current().map_err(|_| self.handle.enter());
            let mut builder = reqwest::ClientBuilder::new();
            #[cfg(not(feature = "js"))]
            {
                builder = builder.connect_timeout(self.connect_timeout);
            }
            builder
        };
        let client = builder.build().context("failed to create reqwest client")?;

        tracing::debug!("sending http request");
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

        tracing::debug!(status=?status, "received http response");

        // Download the body.
        #[cfg(not(feature = "js"))]
        let data = if let Some(timeout_duration) = self.response_body_chunk_timeout {
            // Download the body with a chunk timeout.
            // The timeout prevents long stalls.

            let mut stream = response.bytes_stream();
            let mut buf = Vec::new();

            // Creating tokio timeouts has overhead, so instead of a fresh
            // timeout per chunk a shared timeout is used, and a chunk counter
            // is kept. Only if no chunk was downloaded within the timeout a
            // timeout error is raised.
            'OUTER: loop {
                let timeout = tokio::time::sleep(timeout_duration);
                pin_utils::pin_mut!(timeout);

                let mut chunk_count = 0;

                loop {
                    tokio::select! {
                        // Biased because the timeout is secondary,
                        // and chunks should always have priority.
                        biased;

                        res = stream.try_next() => {
                            match res {
                                Ok(Some(chunk)) => {
                                    buf.extend_from_slice(&chunk);
                                    chunk_count += 1;
                                }
                                Ok(None) => {
                                    break 'OUTER;
                                }
                                Err(e) => {
                                    return Err(e.into());
                                }
                            }
                        }

                        _ = &mut timeout => {
                            if chunk_count == 0 {
                                tracing::warn!(timeout= "timeout while downloading response body");
                                return Err(anyhow::anyhow!("Timeout while downloading response body"));
                            } else {
                                tracing::debug!(downloaded_body_size_bytes=%buf.len(), "download progress");
                                // Timeout, but chunks were downloaded, so
                                // just continue with a fresh timeout.
                                continue 'OUTER;
                            }
                        }
                    }
                }
            }

            buf
        } else {
            response.bytes().await?.to_vec()
        };
        #[cfg(feature = "js")]
        let data = response.bytes().await?.to_vec();

        tracing::debug!(body_size_bytes=%data.len(), "downloaded http response body");

        Ok(HttpResponse {
            status,
            redirected: false,
            body: Some(data),
            headers,
        })
    }
}

impl super::HttpClient for ReqwestHttpClient {
    #[cfg(not(feature = "js"))]
    fn request(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, anyhow::Error>> {
        let client = self.clone();
        let f = async move { client.request(request).await };
        Box::pin(f)
    }

    #[cfg(feature = "js")]
    fn request(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, anyhow::Error>> {
        let client = self.clone();
        let (sender, receiver) = futures::channel::oneshot::channel();
        wasm_bindgen_futures::spawn_local(async move {
            let result = client.request(request).await;
            let _ = sender.send(result);
        });
        Box::pin(async move {
            match receiver.await {
                Ok(result) => result,
                Err(e) => Err(anyhow::Error::new(e)),
            }
        })
    }
}
