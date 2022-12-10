use std::string::FromUtf8Error;

use wasmer_wasi_types::wasix::wasix_http_client_v1::{
    BodyParam, BodyResult, HeaderResult, Request, Response, WasixHttpClientV1,
};

use crate::{http::DynHttpClient, WasiEnv};

pub struct WasixHttpClientImpl {
    env: WasiEnv,
}

impl WasixHttpClientImpl {
    pub fn new(env: WasiEnv) -> Self {
        Self { env }
    }
}

#[derive(Debug)]
pub struct ClientImpl {
    client: DynHttpClient,
}

impl WasixHttpClientV1 for WasixHttpClientImpl {
    type Client = ClientImpl;

    fn client_new(&mut self) -> Result<Self::Client, String> {
        let client = self
            .env
            .runtime
            .http_client()
            .ok_or_else(|| "No http client available".to_string())?
            .clone();
        Ok(ClientImpl { client })
    }

    fn client_send(
        &mut self,
        self_: &Self::Client,
        request: Request<'_>,
    ) -> Result<Response, String> {
        let headers = request
            .headers
            .into_iter()
            .map(|h| {
                let value = String::from_utf8(h.value.to_vec())?;
                Ok((h.key.to_string(), value))
            })
            .collect::<Result<Vec<_>, FromUtf8Error>>()
            .map_err(|_| "non-utf8 request header")?;

        // FIXME: stream body...

        let body = match request.body {
            Some(BodyParam::Fd(_)) => {
                return Err("File descriptor bodies not supported yet".to_string());
            }
            Some(BodyParam::Data(data)) => Some(data.to_vec()),
            None => None,
        };

        let req = crate::http::HttpRequest {
            url: request.url.to_string(),
            method: request.method.to_string(),
            headers,
            body,
            options: crate::http::HttpRequestOptions {
                gzip: false,
                cors_proxy: None,
            },
        };
        let f = self_.client.request(req);

        let res = self.env.tasks.block_on(f).map_err(|e| e.to_string())?;

        let res_headers = res
            .headers
            .into_iter()
            .map(|(key, value)| HeaderResult {
                key,
                value: value.into_bytes(),
            })
            .collect();

        let res_body = if let Some(b) = res.body {
            BodyResult::Data(b)
        } else {
            BodyResult::Data(Vec::new())
        };

        Ok({
            Response {
                status: res.status,
                headers: res_headers,
                body: res_body,
                // TODO: provide redirect urls?
                redirect_urls: None,
            }
        })
    }
}
