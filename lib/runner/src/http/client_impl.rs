use std::sync::Arc;

use http::{HeaderMap, HeaderValue};
use url::Url;

use crate::{
    bindings::wasix_http_client_v1 as sys,
    capabilities::Capabilities,
    http::{DynHttpClient, HttpClientCapabilityV1},
    task_manager::InlineWaker,
    Runtime, WasiEnv,
};

impl std::fmt::Display for sys::Method<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let v = match self {
            sys::Method::Get => "GET",
            sys::Method::Head => "HEAD",
            sys::Method::Post => "POST",
            sys::Method::Put => "PUT",
            sys::Method::Delete => "DELETE",
            sys::Method::Connect => "CONNECT",
            sys::Method::Options => "OPTIONS",
            sys::Method::Trace => "TRACE",
            sys::Method::Patch => "PATCH",
            sys::Method::Other(other) => *other,
        };
        write!(f, "{v}")
    }
}

pub struct WasixHttpClientImpl {
    cap: Capabilities,
    runtime: Arc<dyn Runtime + Send + Sync>,
}

impl WasixHttpClientImpl {
    pub fn new(env: &WasiEnv) -> Self {
        Self {
            // TODO: Should be a shared reference
            // Currently this client would not adapt to changes in the capabilities.
            cap: env.capabilities.clone(),
            runtime: env.runtime.clone(),
        }
    }
}

#[derive(Debug)]
pub struct ClientImpl {
    client: DynHttpClient,
    capabilities: HttpClientCapabilityV1,
}

impl sys::WasixHttpClientV1 for WasixHttpClientImpl {
    type Client = ClientImpl;

    fn client_new(&mut self) -> Result<Self::Client, String> {
        let capabilities = if self.cap.insecure_allow_all {
            HttpClientCapabilityV1::new_allow_all()
        } else if !self.cap.http_client.is_deny_all() {
            self.cap.http_client.clone()
        } else {
            return Err("Permission denied - http client not enabled".to_string());
        };

        let client = self
            .runtime
            .http_client()
            .ok_or_else(|| "No http client available".to_string())?
            .clone();
        Ok(ClientImpl {
            client,
            capabilities,
        })
    }

    fn client_send(
        &mut self,
        self_: &Self::Client,
        request: sys::Request<'_>,
    ) -> Result<sys::Response, String> {
        let uri: http::Uri = request
            .url
            .parse()
            .map_err(|err| format!("Invalid request url: {err}"))?;
        let host = uri.host().unwrap_or_default();
        if !self_.capabilities.can_access_domain(host) {
            return Err(format!(
                "Permission denied: http capability not enabled for host '{host}'"
            ));
        }

        let headers = request
            .headers
            .into_iter()
            .map(|h| {
                let value = HeaderValue::from_bytes(h.value).map_err(|e| e.to_string())?;
                let key =
                    http::HeaderName::from_bytes(h.key.as_bytes()).map_err(|e| e.to_string())?;
                Ok((key, value))
            })
            .collect::<Result<HeaderMap, String>>()?;

        // FIXME: stream body...

        let body = match request.body {
            Some(sys::BodyParam::Fd(_)) => {
                return Err("File descriptor bodies not supported yet".to_string());
            }
            Some(sys::BodyParam::Data(data)) => Some(data.to_vec()),
            None => None,
        };

        let method = match request.method {
            sys::Method::Get => http::Method::GET,
            sys::Method::Head => http::Method::HEAD,
            sys::Method::Post => http::Method::POST,
            sys::Method::Put => http::Method::PUT,
            sys::Method::Delete => http::Method::DELETE,
            sys::Method::Connect => http::Method::CONNECT,
            sys::Method::Options => http::Method::OPTIONS,
            sys::Method::Trace => http::Method::TRACE,
            sys::Method::Patch => http::Method::PATCH,
            sys::Method::Other(other) => return Err(format!("Unknown method: {other}")),
        };

        let req = crate::http::HttpRequest {
            url: Url::parse(request.url).map_err(|e| e.to_string())?,
            method,
            headers,
            body,
            options: crate::http::HttpRequestOptions {
                gzip: false,
                cors_proxy: None,
            },
        };
        let f = self_.client.request(req);
        let res = InlineWaker::block_on(f).map_err(|e| e.to_string())?;

        let res_headers = res
            .headers
            .iter()
            .map(|(name, value)| sys::HeaderResult {
                key: name.to_string(),
                value: value.as_bytes().to_vec(),
            })
            .collect();

        let res_body = if let Some(b) = res.body {
            sys::BodyResult::Data(b)
        } else {
            sys::BodyResult::Data(Vec::new())
        };

        Ok({
            sys::Response {
                status: res.status.as_u16(),
                headers: res_headers,
                body: res_body,
                // TODO: provide redirect urls?
                redirect_urls: None,
            }
        })
    }
}
