use std::time::Duration;

use anyhow::Context;
use futures::{TryStreamExt, future::BoxFuture};
use std::convert::TryFrom;
use tokio::runtime::Handle;

use super::{HttpDownloadObserver, HttpRequest, HttpResponse};

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
    async fn request(
        &self,
        request: HttpRequest,
        progress: Option<HttpDownloadObserver>,
    ) -> Result<HttpResponse, anyhow::Error> {
        let method = reqwest::Method::try_from(request.method.as_str())
            .with_context(|| format!("Invalid http method {}", request.method))?;

        // TODO: use persistent client?
        let builder = {
            let _guard = Handle::try_current().map_err(|_| self.handle.enter());
            let mut builder = reqwest::ClientBuilder::new();
            #[cfg(not(feature = "js"))]
            {
                builder = builder
                    .connect_timeout(self.connect_timeout)
                    .gzip(progress.is_some())
                    .zstd(progress.is_some());
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
        let data = if let Some(progress) = &progress {
            let total = if headers.contains_key(http::header::CONTENT_ENCODING) {
                None
            } else {
                response.content_length()
            };
            let decoded = !headers.contains_key(http::header::CONTENT_ENCODING);
            let mut stream = response.bytes_stream();
            let mut buf = Vec::new();
            if decoded {
                progress(0, total, false);
            }
            loop {
                let chunk = match self.response_body_chunk_timeout {
                    Some(timeout) => tokio::time::timeout(timeout, stream.try_next())
                        .await
                        .context("Timeout while downloading response body")??,
                    None => stream.try_next().await?,
                };
                let Some(chunk) = chunk else { break };
                buf.extend_from_slice(&chunk);
                if decoded {
                    progress(buf.len() as u64, total, false);
                }
            }
            buf
        } else if let Some(timeout_duration) = self.response_body_chunk_timeout {
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
    fn request_with_progress(
        &self,
        request: HttpRequest,
        progress: HttpDownloadObserver,
    ) -> BoxFuture<'_, Result<HttpResponse, anyhow::Error>> {
        let client = self.clone();
        Box::pin(async move { client.request(request, Some(progress)).await })
    }

    #[cfg(not(feature = "js"))]
    fn request(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, anyhow::Error>> {
        let client = self.clone();
        let f = async move { client.request(request, None).await };
        Box::pin(f)
    }

    #[cfg(feature = "js")]
    fn request(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, anyhow::Error>> {
        let client = self.clone();
        let (sender, receiver) = futures::channel::oneshot::channel();
        wasm_bindgen_futures::spawn_local(async move {
            let result = client.request(request, None).await;
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::http::HttpClient;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::{Arc, Mutex},
    };

    // A real socket catches buffering and content-encoding mistakes that a
    // mocked HttpClient cannot detect. Delay chunks to force intermediate reads.
    fn serve(
        bytes: Vec<u8>,
        encoding: Option<&str>,
        truncated: bool,
    ) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/package", listener.local_addr().unwrap());
        let encoding = encoding
            .map(|e| format!("Content-Encoding: {e}\r\n"))
            .unwrap_or_default();
        let task = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = [0u8; 8192];
            socket.read(&mut request).unwrap();
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n{}Connection: close\r\n\r\n",
                bytes.len() + usize::from(truncated),
                encoding
            )
            .unwrap();
            for chunk in bytes.chunks((bytes.len() / 4).max(1)) {
                if socket.write_all(chunk).is_err() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(30));
            }
        });
        (url, task)
    }

    #[tokio::test]
    async fn progress_streams_decoded_bytes_for_identity_gzip_and_zstd() {
        let body: Vec<u8> = (0..256 * 1024).map(|i| (i * 71 % 251) as u8).collect();
        let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gzip.write_all(&body).unwrap();
        let gzip = gzip.finish().unwrap();
        let zstd = zstd::stream::encode_all(body.as_slice(), 3).unwrap();
        for (encoded, encoding) in [
            (body.clone(), None),
            (gzip, Some("gzip")),
            (zstd, Some("zstd")),
        ] {
            let (url, server) = serve(encoded, encoding, false);
            let updates = Arc::new(Mutex::new(Vec::new()));
            let captured = updates.clone();
            let request = http::Request::get(url).body(()).unwrap().into();
            let response = HttpClient::request_with_progress(
                &ReqwestHttpClient::default(),
                request,
                Arc::new(move |received, total, cached| {
                    assert!(!cached);
                    captured.lock().unwrap().push((received, total));
                }),
            )
            .await
            .unwrap();
            server.join().unwrap();
            assert_eq!(response.body.unwrap(), body);
            let updates = updates.lock().unwrap();
            assert_eq!(updates.first().unwrap().0, 0);
            assert_eq!(updates.last().unwrap().0, body.len() as u64);
            assert!(updates.windows(2).all(|p| p[0].0 <= p[1].0));
            if encoding.is_none() {
                assert!(updates.iter().any(|p| p.0 > 0 && p.0 < body.len() as u64));
                assert!(updates.iter().all(|p| p.1 == Some(body.len() as u64)));
            } else {
                assert!(
                    updates.iter().all(|p| p.1.is_none()),
                    "compressed wire length is not the decoded total"
                );
            }
        }
    }

    #[tokio::test]
    async fn truncated_body_is_an_error() {
        let (url, server) = serve(vec![42; 100], None, true);
        let request = http::Request::get(url).body(()).unwrap().into();
        let result = HttpClient::request_with_progress(
            &ReqwestHttpClient::default(),
            request,
            Arc::new(|_, _, _| {}),
        )
        .await;
        server.join().unwrap();
        assert!(result.is_err());
    }
}
