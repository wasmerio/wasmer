use anyhow::{Context, Error};
use futures::future::BoxFuture;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{RequestInit, RequestMode, Window, WorkerGlobalScope};

use crate::{
    http::{HttpRequest, HttpRequestOptions, HttpResponse},
    utils::web::js_error,
};

#[derive(Debug, Default, Clone, PartialEq)]
pub struct WebHttpClient {}

impl super::HttpClient for WebHttpClient {
    fn request(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, Error>> {
        let (sender, receiver) = futures::channel::oneshot::channel();

        wasm_bindgen_futures::spawn_local(async move {
            let result = fetch(request).await;
            sender.send(result);
        });

        Box::pin(async move {
            match receiver.await {
                Ok(result) => result,
                Err(e) => Err(Error::new(e)),
            }
        })
    }
}

/// Send a `fetch()` request.
async fn fetch(request: HttpRequest) -> Result<HttpResponse, Error> {
    let HttpRequest {
        url,
        method,
        headers,
        body,
        options: HttpRequestOptions {
            gzip: _,
            cors_proxy,
        },
    } = request;

    let mut opts = RequestInit::new();
    opts.method(method.as_str());
    opts.mode(RequestMode::Cors);

    if let Some(data) = body {
        let data_len = data.len();
        let array = js_sys::Uint8Array::new_with_length(data_len as u32);
        array.copy_from(&data[..]);

        opts.body(Some(&array));
    }

    let request = {
        let request = web_sys::Request::new_with_str_and_init(url.as_str(), &opts)
            .map_err(js_error)
            .context("Could not construct request object")?;

        let set_headers = request.headers();
        for (name, val) in headers.iter() {
            let val = String::from_utf8_lossy(val.as_bytes());
            set_headers
                .set(name.as_str(), &val)
                .map_err(js_error)
                .with_context(|| format!("could not apply request header: '{name}': '{val}'"))?;
        }
        request
    };

    let resp_value = match call_fetch(&request).await {
        Ok(a) => a,
        Err(e) => {
            // If the request failed it may be because of CORS so if a cors proxy
            // is configured then try again with the cors proxy
            let url_store;
            let url = if let Some(cors_proxy) = cors_proxy {
                url_store = format!("https://{}/{}", cors_proxy, url);
                url_store.as_str()
            } else {
                return Err(js_error(e).context(format!("Could not fetch '{url}'")));
            };

            let request = web_sys::Request::new_with_str_and_init(url, &opts)
                .map_err(js_error)
                .with_context(|| format!("Could not construct request for url '{url}'"))?;

            let set_headers = request.headers();
            for (name, val) in headers.iter() {
                let value = String::from_utf8_lossy(val.as_bytes());
                set_headers
                    .set(name.as_str(), &value)
                    .map_err(js_error)
                    .with_context(|| {
                        anyhow::anyhow!("Could not apply request header: '{name}': '{value}'")
                    })?;
            }

            call_fetch(&request)
                .await
                .map_err(js_error)
                .with_context(|| format!("Could not fetch '{url}'"))?
        }
    };
    assert!(resp_value.is_instance_of::<web_sys::Response>());
    let resp: web_sys::Response = resp_value.dyn_into().unwrap();

    todo!()
}

fn call_fetch(request: &web_sys::Request) -> JsFuture {
    let global = js_sys::global();
    if JsValue::from_str("WorkerGlobalScope").js_in(&global)
        && global.is_instance_of::<WorkerGlobalScope>()
    {
        JsFuture::from(
            global
                .unchecked_into::<WorkerGlobalScope>()
                .fetch_with_request(request),
        )
    } else {
        JsFuture::from(
            global
                .unchecked_into::<Window>()
                .fetch_with_request(request),
        )
    }
}
