use anyhow::Context;
use futures::future::BoxFuture;
use std::convert::TryFrom;

use super::{HttpRequest, HttpResponse};

#[derive(Default, Clone, Debug)]
pub struct ReqwestHttpClient {}

impl ReqwestHttpClient {
    async fn request(&self, request: HttpRequest) -> Result<HttpResponse, anyhow::Error> {
        let method = reqwest::Method::try_from(request.method.as_str())
            .with_context(|| format!("Invalid http method {}", request.method))?;

        // TODO: use persistent client?
        let client = reqwest::ClientBuilder::default()
            .build()
            .context("Could not create reqwest client")?;

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

        let response = client.execute(request).await?;

        let status = response.status().as_u16();
        let status_text = response.status().as_str().to_string();
        // TODO: prevent redundant header copy.
        let headers = response
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap().to_string()))
            .collect();
        let data = response.bytes().await?.to_vec();

        Ok(HttpResponse {
            pos: 0usize,
            ok: true,
            status,
            status_text,
            redirected: false,
            body: Some(data),
            headers,
        })
    }
}

impl super::HttpClient for ReqwestHttpClient {
    fn request(&self, request: HttpRequest) -> BoxFuture<Result<HttpResponse, anyhow::Error>> {
        let client = self.clone();
        let f = async move { client.request(request).await };
        Box::pin(f)
    }
}
