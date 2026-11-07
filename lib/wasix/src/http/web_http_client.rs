use std::sync::Arc;

use anyhow::{Context, Error};
use futures::{channel::oneshot, future::BoxFuture};
use http::header::{HeaderMap, HeaderValue, IntoHeaderName};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{RequestInit, RequestMode, Window, WorkerGlobalScope};

use crate::{
    VirtualTaskManager, WasiThreadError,
    http::{HttpClient, HttpDownloadObserver, HttpRequest, HttpRequestOptions, HttpResponse},
    utils::web::js_error,
};

#[derive(Debug, Default, Clone)]
#[non_exhaustive]
pub struct WebHttpClient {
    default_headers: HeaderMap,
    tasks: Option<Arc<dyn VirtualTaskManager + Send + Sync>>,
}

impl WebHttpClient {
    pub fn new() -> Self {
        WebHttpClient {
            default_headers: HeaderMap::new(),
            tasks: None,
        }
    }

    pub fn with_task_manager(
        &mut self,
        tasks: Arc<dyn VirtualTaskManager + Send + Sync>,
    ) -> &mut Self {
        self.tasks = Some(tasks);
        self
    }

    pub fn with_default_header(
        &mut self,
        name: impl IntoHeaderName,
        value: HeaderValue,
    ) -> &mut Self {
        self.default_headers.insert(name, value);
        self
    }

    pub fn extend_default_headers(&mut self, map: HeaderMap) -> &mut Self {
        self.default_headers.extend(map);
        self
    }

    /// Execute a `fetch()` request.
    ///
    /// # Hack
    ///
    /// This function is here to work around issues that are associated with the
    /// promises we get when calling into JavaScript.
    ///
    /// Under most conditions it's perfectly fine to call
    /// [`wasm_bindgen_futures::spawn_local()`] directly from the
    /// [`WebHttpClient::request()`] method, however there is a pretty nasty
    /// gotcha when it comes to syscalls.
    ///
    /// - The Wasmer VM doesn't support `async` host functions, so all
    ///   syscalls must block
    /// - Some syscalls need to run asynchronous operations
    /// - To call async code from a sync function, we use
    ///   [`crate::syscalls::__asyncify_light()`] which uses
    ///   [`futures::executor::block_on()`] to poll the future on the current
    ///   thread until it resolves
    /// - In order for a [`wasm_bindgen_futures::spawn_local()`] future (i.e.
    ///   `fetch()`) to run to completion, you need to return control to the
    ///   JavaScript event loop
    ///
    /// This causes a nasty deadlock where the syscall won't return until the
    /// `fetch()` promise resolves, but the `fetch()` promise can't resolve
    /// until the syscall returns.
    ///
    /// We saw one example of this when a WASIX program uses the `wasmer run
    /// ...` virtual command and the [`crate::Runtime`] needs to load a package
    /// from the registry.
    ///
    /// The workaround is use a [`VirtualTaskManager`] to run `fetch()` on a
    /// background thread and hope *that* thread doesn't get stuck in a
    /// deadlock. Otherwise if no [`VirtualTaskManager`] was provided, we'll run
    /// the `fetch()` request on the current event loop and hope for the best.
    fn spawn_js(
        &self,
        request: HttpRequest,
        progress: Option<HttpDownloadObserver>,
    ) -> Result<oneshot::Receiver<Result<HttpResponse, Error>>, WasiThreadError> {
        let (sender, receiver) = oneshot::channel();

        fn spawn_fetch(
            request: HttpRequest,
            mut sender: oneshot::Sender<Result<HttpResponse, Error>>,
            progress: Option<HttpDownloadObserver>,
        ) {
            wasm_bindgen_futures::spawn_local(async move {
                // Dropping the receiver cancels the local fetch future, whose
                // AbortController also aborts any pending response body read.
                let result = {
                    let fetch = Box::pin(fetch(request, progress));
                    let cancelled = Box::pin(sender.cancellation());
                    match futures::future::select(fetch, cancelled).await {
                        futures::future::Either::Left((result, _)) => Some(result),
                        futures::future::Either::Right(_) => None,
                    }
                };
                if let Some(result) = result {
                    let _ = sender.send(result);
                }
            });
        }

        match self.tasks.as_deref() {
            Some(tasks) => {
                tasks.task_shared(Box::new(move || {
                    Box::pin(async move {
                        spawn_fetch(request, sender, progress);
                    })
                }))?;
            }
            None => {
                spawn_fetch(request, sender, progress);
            }
        }

        Ok(receiver)
    }
}

impl HttpClient for WebHttpClient {
    fn request_with_progress(
        &self,
        mut request: HttpRequest,
        progress: HttpDownloadObserver,
    ) -> BoxFuture<'_, Result<HttpResponse, Error>> {
        for (name, value) in &self.default_headers {
            if !request.headers.contains_key(name) {
                request.headers.insert(name, value.clone());
            }
        }
        let receiver = self.spawn_js(request, Some(progress));
        Box::pin(async move { receiver?.await.map_err(Error::new)? })
    }

    fn request(&self, mut request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, Error>> {
        for (name, value) in &self.default_headers {
            if !request.headers.contains_key(name) {
                request.headers.insert(name, value.clone());
            }
        }

        let receiver = self.spawn_js(request, None);

        Box::pin(async move {
            match receiver?.await {
                Ok(result) => result,
                Err(e) => Err(Error::new(e)),
            }
        })
    }
}

/// Send a `fetch()` request using the browser APIs.
async fn fetch(
    request: HttpRequest,
    progress: Option<HttpDownloadObserver>,
) -> Result<HttpResponse, Error> {
    struct AbortOnDrop(web_sys::AbortController);
    impl Drop for AbortOnDrop {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    let controller = AbortOnDrop(web_sys::AbortController::new().map_err(js_error)?);
    let HttpRequest {
        url,
        method,
        mut headers,
        body,
        options: HttpRequestOptions {
            gzip: _,
            cors_proxy,
        },
    } = request;

    // Let Fetch supply the browser's User-Agent. An explicit value triggers a
    // CORS preflight in Firefox/WebKit, unlike Chromium which drops it. Package
    // CDNs may allow cross-origin GETs without supporting those preflights.
    headers.remove(http::header::USER_AGENT);

    let mut opts = RequestInit::new();
    opts.set_signal(Some(&controller.0.signal()));
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
            let url = if let Some(cors_proxy) = cors_proxy {
                format!("https://{}/{}", cors_proxy, url)
            } else {
                return Err(js_error(e).context(format!("Could not fetch '{url}'")));
            };

            let request = web_sys::Request::new_with_str_and_init(&url, &opts)
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

    let response = resp_value.dyn_ref().unwrap();
    read_response(response, progress).await
}

async fn read_response(
    response: &web_sys::Response,
    progress: Option<HttpDownloadObserver>,
) -> Result<HttpResponse, anyhow::Error> {
    let status = http::StatusCode::from_u16(response.status())?;
    let headers = headers(response.headers()).context("Unable to read the headers")?;
    // The iOS loopback package transport owns a verified disk cache. It marks
    // cache hits so local delivery is not misreported as a network download.
    let cached = headers
        .get("x-wasmer-package-cache")
        .is_some_and(|v| v == "hit");
    let body = match response.body() {
        Some(body) => read_body(body, content_length(response), cached, progress.as_ref()).await?,
        None => Vec::new(),
    };

    Ok(HttpResponse {
        body: Some(body),
        redirected: response.redirected(),
        status,
        headers,
    })
}

/// Read a response body chunk by chunk into one buffer.
///
/// Bodies such as package images are tens of megabytes. `arrayBuffer()` would
/// also hold the whole body in a JavaScript buffer, and a buffer grown by
/// doubling reaches up to twice the body size. Large freed blocks like these
/// stay committed in the host and Wasm allocators long after the download.
async fn read_body(
    body: web_sys::ReadableStream,
    size_hint: Option<usize>,
    cached: bool,
    progress: Option<&HttpDownloadObserver>,
) -> Result<Vec<u8>, anyhow::Error> {
    let reader: web_sys::ReadableStreamDefaultReader = body.get_reader().unchecked_into();
    let mut bytes = Vec::new();
    if let Some(size) = size_hint {
        // Only a hint: an absurd length must not abort, and growth still works.
        let _ = bytes.try_reserve_exact(size.min(MAX_BODY_PRESIZE));
    }
    loop {
        let next = JsFuture::from(reader.read()).await.map_err(js_error)?;
        if js_sys::Reflect::get(&next, &"done".into())
            .map_err(js_error)?
            .as_bool()
            == Some(true)
        {
            break;
        }
        // Fetch yields Uint8Array chunks: use them in place. Passing one to
        // `Uint8Array::new` would copy it.
        let chunk = js_sys::Reflect::get(&next, &"value".into())
            .map_err(js_error)?
            .dyn_into::<js_sys::Uint8Array>()
            .unwrap_or_else(|value| js_sys::Uint8Array::new(&value));
        let start = bytes.len();
        let needed = start + chunk.length() as usize;
        if needed > bytes.capacity() {
            bytes.reserve_exact(grown_capacity(bytes.capacity(), needed, size_hint) - start);
        }
        bytes.resize(needed, 0);
        chunk.copy_to(&mut bytes[start..]);
        if let Some(progress) = progress {
            progress(
                if cached { 0 } else { bytes.len() as u64 },
                cached.then_some(0),
                cached,
            );
        }
    }
    reader.release_lock();
    // Callers keep the body, often as `Bytes`, which retains spare capacity.
    bytes.shrink_to_fit();
    Ok(bytes)
}

/// The most a body's `Content-Length` may reserve before the body arrives.
///
/// The header is untrusted and Wasm memory never shrinks, so a larger hint
/// must not grow memory by itself. Package images such as `wasmer/edgejs`
/// (78 MB) fit within it.
const MAX_BODY_PRESIZE: usize = 128 * 1024 * 1024;

/// The capacity a body buffer of `capacity` grows to so it can hold `needed`
/// bytes.
///
/// Like `Vec`'s own growth, it at most doubles, so a size hint beyond
/// [`MAX_BODY_PRESIZE`] reserves memory only as fast as the body arrives. It
/// stops at the hint while the hint still covers the body, so an accurate
/// length above the limit still ends in an exactly sized buffer.
fn grown_capacity(capacity: usize, needed: usize, size_hint: Option<usize>) -> usize {
    let doubled = needed.max(capacity.saturating_mul(2));
    match size_hint {
        Some(hint) if hint >= needed => doubled.min(hint),
        _ => doubled,
    }
}

/// The visible `Content-Length`, used only to size the body buffer: Fetch
/// decodes compressed bodies, and cross-origin responses can hide
/// `Content-Encoding`, so it may not be the decoded size.
fn content_length(response: &web_sys::Response) -> Option<usize> {
    response
        .headers()
        .get("content-length")
        .ok()
        .flatten()?
        .trim()
        .parse()
        .ok()
}

fn headers(headers: web_sys::Headers) -> Result<http::HeaderMap, anyhow::Error> {
    let iter = js_sys::try_iter(&headers)
        .map_err(js_error)?
        .context("Not an iterator")?;
    let mut header_map = http::HeaderMap::new();

    for pair in iter {
        let pair = pair.map_err(js_error)?;
        let [key, value]: [js_sys::JsString; 2] =
            js_array(&pair).context("Unable to unpack the header's key-value pairs")?;

        let key = String::from(key);
        let key: http::HeaderName = key.parse()?;
        let value = String::from(value);
        let value = http::HeaderValue::from_str(&value)
            .with_context(|| format!("Invalid header value: {value}"))?;

        header_map.insert(key, value);
    }

    // Fetch already decoded the body, but retains the wire encoding/length
    // headers. Do not pass those on to consumers that would decode it again.
    header_map.remove(http::header::CONTENT_ENCODING);
    header_map.remove(http::header::CONTENT_LENGTH);
    Ok(header_map)
}

fn js_array<T, const N: usize>(value: &JsValue) -> Result<[T; N], anyhow::Error>
where
    T: JsCast,
{
    let array: &js_sys::Array = value.dyn_ref().context("Not an array")?;

    let mut items = Vec::new();

    for value in array.iter() {
        let item = value
            .dyn_into()
            .map_err(|_| anyhow::anyhow!("Unable to cast to a {}", std::any::type_name::<T>()))?;
        items.push(item);
    }

    <[T; N]>::try_from(items).map_err(|original| {
        anyhow::anyhow!(
            "Unable to turn a list of {} items into an array of {N} items",
            original.len()
        )
    })
}

pub async fn get_response_data(resp: &web_sys::Response) -> Result<Vec<u8>, anyhow::Error> {
    let buffer = JsFuture::from(resp.array_buffer().unwrap())
        .await
        .map_err(js_error)
        .with_context(|| "Could not retrieve response body".to_string())?;

    let buffer = js_sys::Uint8Array::new(&buffer);

    Ok(buffer.to_vec())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::resolver::BackendSource;

    #[wasm_bindgen_test::wasm_bindgen_test]
    fn decoded_fetch_headers_do_not_describe_compressed_bytes() {
        for encoding in ["gzip", "br", "zstd", "gzip, zstd"] {
            let wire_headers = web_sys::Headers::new().unwrap();
            wire_headers.set("content-encoding", encoding).unwrap();
            wire_headers.set("content-length", "123").unwrap();
            wire_headers
                .set("content-type", "application/webc")
                .unwrap();
            let decoded = headers(wire_headers).unwrap();
            assert!(!decoded.contains_key(http::header::CONTENT_ENCODING));
            assert!(!decoded.contains_key(http::header::CONTENT_LENGTH));
            assert_eq!(decoded[http::header::CONTENT_TYPE], "application/webc");
        }
        // CORS can hide Content-Encoding while exposing Content-Length.
        let wire_headers = web_sys::Headers::new().unwrap();
        wire_headers.set("content-length", "123").unwrap();
        assert!(headers(wire_headers).unwrap().is_empty());
    }

    #[wasm_bindgen_test::wasm_bindgen_test]
    fn body_growth_follows_the_data_not_the_hint() {
        const MIB: usize = 1024 * 1024;
        // Without a hint, the buffer doubles like a `Vec`.
        assert_eq!(grown_capacity(0, 10, None), 10);
        assert_eq!(grown_capacity(10, 11, None), 20);
        assert_eq!(grown_capacity(10, 50, None), 50);
        // A hint caps growth while the body still fits in it...
        assert_eq!(grown_capacity(10, 11, Some(15)), 15);
        assert_eq!(grown_capacity(10, 15, Some(15)), 15);
        // ...but a hint the body has outgrown is ignored.
        assert_eq!(grown_capacity(15, 16, Some(15)), 30);
        // An accurate length above the presize limit ends exactly sized.
        let length = 300 * MIB;
        let mut capacity = MAX_BODY_PRESIZE;
        while capacity < length {
            capacity = grown_capacity(capacity, capacity + 1, Some(length));
            assert!(capacity <= length);
        }
        assert_eq!(capacity, length);
        // An exaggerated length never reserves more than twice the data.
        let received = MAX_BODY_PRESIZE + 1;
        let capacity = grown_capacity(MAX_BODY_PRESIZE, received, Some(usize::MAX));
        assert!(capacity <= 2 * received);
    }

    #[wasm_bindgen_test::wasm_bindgen_test]
    async fn query_the_wasmer_registry_graphql_endpoint() {
        let http_client = WebHttpClient::default();
        let query = r#"{
            "query": "{ info { defaultFrontend } }"
        }"#;
        let request = http::Request::post(BackendSource::WASMER_PROD_ENDPOINT)
            .header(http::header::CONTENT_TYPE, "application/json")
            .body(query)
            .unwrap();

        let response = http_client.request(request.into()).await.unwrap();

        assert_eq!(
            response
                .headers
                .get(http::header::CONTENT_TYPE)
                .unwrap()
                .to_str()
                .unwrap(),
            "application/json",
        );
        let body: serde_json::Value =
            serde_json::from_slice(response.body.as_deref().unwrap()).unwrap();
        let frontend_url = body
            .pointer("/data/info/defaultFrontend")
            .unwrap()
            .as_str()
            .unwrap();
        assert_eq!(frontend_url, "https://wasmer.io");
    }
}
