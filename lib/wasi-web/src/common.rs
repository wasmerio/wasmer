use std::cell::Cell;

use js_sys::Function;
#[allow(unused_imports, dead_code)]
use tracing::{debug, error, info, trace, warn};
use wasm_bindgen::{prelude::*, JsCast};
use wasm_bindgen_futures::*;
use web_sys::*;

#[wasm_bindgen]
#[derive(Default)]
pub struct AnimationFrameCallbackWrapper {
    // These are both boxed because we want stable addresses!
    handle: Box<Cell<Option<i32>>>,
    func: Option<Box<dyn FnMut() -> bool + 'static>>,
}

#[allow(clippy::option_map_unit_fn)]
impl Drop for AnimationFrameCallbackWrapper {
    fn drop(&mut self) {
        self.handle.get().map(cancel_animation_frame);
    }
}

pub(crate) fn set_panic_hook() {
    console_error_panic_hook::set_once();
}

pub(crate) fn cancel_animation_frame(handle: i32) {
    debug!("Cancelling {}..", handle);

    web_sys::window()
        .unwrap()
        .cancel_animation_frame(handle)
        .unwrap()
}

impl AnimationFrameCallbackWrapper /*<'a>*/ {
    pub fn new() -> Self {
        Self {
            handle: Box::new(Cell::new(None)),
            func: None,
        }
    }

    pub fn leak(self) -> &'static mut Self {
        Box::leak(Box::new(self))
    }

    /// To use this, you'll probably have to leak the wrapper.
    ///
    /// `Self::leak` can help you with this.
    pub fn safe_start(&'static mut self, func: impl FnMut() -> bool + 'static) {
        unsafe { self.inner(func) }
    }

    /// This is extremely prone to crashing and is probably unsound; use at your
    /// own peril.
    #[inline(never)]
    pub unsafe fn start<'s, 'f: 's>(&'s mut self, func: impl FnMut() -> bool + 'f) {
        debug!(""); // load bearing, somehow...
        self.inner(func)
    }

    #[allow(unused_unsafe, clippy::borrowed_box)]
    unsafe fn inner<'s, 'f: 's>(&'s mut self, func: impl FnMut() -> bool + 'f) {
        if let Some(handle) = self.handle.get() {
            cancel_animation_frame(handle)
        }

        let func: Box<dyn FnMut() -> bool + 'f> = Box::new(func);
        // Crime!
        let func: Box<dyn FnMut() -> bool + 'static> = unsafe { core::mem::transmute(func) };
        self.func = Some(func);

        // This is the dangerous part; we're saying this is okay because we
        // cancel the RAF on Drop of this structure so, in theory, when the
        // function goes out of scope, the RAF will also be cancelled and the
        // invalid reference won't be used.
        let wrapper: &'static mut Self = unsafe { core::mem::transmute(self) };

        let window = web_sys::window().unwrap();

        fn recurse(
            f: &'static mut Box<dyn FnMut() -> bool + 'static>,
            h: &'static Cell<Option<i32>>,
            window: Window,
        ) -> Function {
            let val = Closure::once_into_js(move || {
                // See: https://github.com/rust-lang/rust/issues/42574
                let f = f;

                if h.get().is_none() {
                    warn!("you should never see this...");
                    return;
                }

                if (f)() {
                    let next = recurse(f, h, window.clone());
                    let id = window.request_animation_frame(&next).unwrap();
                    h.set(Some(id));
                } else {
                    // No need to drop the function here, really.
                    // It'll get dropped whenever the wrapper gets dropped.
                    // drop(w.func.take());

                    // We should remove the handle though, so that when the
                    // wrapper gets dropped it doesn't try to cancel something
                    // that already ran.
                    let _ = h.take();
                }
            });

            val.dyn_into().unwrap()
        }

        let func: &'static mut Box<dyn FnMut() -> bool + 'static> = wrapper.func.as_mut().unwrap();
        let starting = recurse(func, &wrapper.handle, window.clone());
        wrapper
            .handle
            .set(Some(window.request_animation_frame(&starting).unwrap()));
    }
}

fn fetch_internal(request: &Request) -> JsFuture {
    if is_worker() {
        let global = js_sys::global();
        JsFuture::from(
            global
                .dyn_into::<WorkerGlobalScope>()
                .unwrap()
                .fetch_with_request(request),
        )
    } else {
        JsFuture::from(web_sys::window().unwrap().fetch_with_request(request))
    }
}

pub async fn fetch(
    url: &str,
    method: &str,
    _gzip: bool,
    cors_proxy: Option<String>,
    headers: Vec<(String, String)>,
    data: Option<Vec<u8>>,
) -> Result<Response, anyhow::Error> {
    let mut opts = RequestInit::new();
    opts.method(method);
    opts.mode(RequestMode::Cors);

    if let Some(data) = data {
        let data_len = data.len();
        let array = js_sys::Uint8Array::new_with_length(data_len as u32);
        array.copy_from(&data[..]);

        opts.body(Some(&array));
    }

    let request = {
        let request = Request::new_with_str_and_init(&url, &opts)
            .map_err(|_| anyhow::anyhow!("Could not construct request object"))?;

        let set_headers = request.headers();
        for (name, val) in headers.iter() {
            set_headers.set(name.as_str(), val.as_str()).map_err(|_| {
                anyhow::anyhow!("could not apply request header: '{name}': '{val}'")
            })?;
        }
        request
    };

    let resp_value = match fetch_internal(&request).await.ok() {
        Some(a) => a,
        None => {
            // If the request failed it may be because of CORS so if a cors proxy
            // is configured then try again with the cors proxy
            let url_store;
            let url = if let Some(cors_proxy) = cors_proxy {
                url_store = format!("https://{}/{}", cors_proxy, url);
                url_store.as_str()
            } else {
                // TODO: more descriptive error.
                return Err(anyhow::anyhow!("Could not fetch '{url}'"));
            };

            let request = Request::new_with_str_and_init(url, &opts)
                .map_err(|_| anyhow::anyhow!("Could not construct request for url '{url}'"))?;

            let set_headers = request.headers();
            for (name, val) in headers.iter() {
                set_headers.set(name.as_str(), val.as_str()).map_err(|_| {
                    anyhow::anyhow!("Could not apply request header: '{name}': '{val}'")
                })?;
            }

            fetch_internal(&request).await.map_err(|_| {
                // TODO: more descriptive error.
                anyhow::anyhow!("Could not fetch '{url}'")
            })?
        }
    };
    assert!(resp_value.is_instance_of::<Response>());
    let resp: Response = resp_value.dyn_into().unwrap();

    if resp.status() < 200 || resp.status() >= 400 {
        debug!("fetch-failed: {}", resp.status_text());
        return Err(anyhow::anyhow!(
            "Request to '{url}' failed with status {}",
            resp.status()
        ));
    }

    Ok(resp)
}

/*
pub async fn fetch_data(
    url: &str,
    method: &str,
    gzip: bool,
    cors_proxy: Option<String>,
    headers: Vec<(String, String)>,
    data: Option<Vec<u8>>,
) -> Result<Vec<u8>, u32> {
    Ok(get_response_data(fetch(url, method, gzip, cors_proxy, headers, data).await?).await?)
}
*/

pub async fn get_response_data(resp: Response) -> Result<Vec<u8>, anyhow::Error> {
    let resp = { JsFuture::from(resp.array_buffer().unwrap()) };

    let arrbuff_value = resp.await.map_err(|_| {
        // TODO: forward error message
        anyhow::anyhow!("Could not retrieve response body")
    })?;
    assert!(arrbuff_value.is_instance_of::<js_sys::ArrayBuffer>());
    //let arrbuff: js_sys::ArrayBuffer = arrbuff_value.dyn_into().unwrap();

    let typebuff: js_sys::Uint8Array = js_sys::Uint8Array::new(&arrbuff_value);
    let ret = typebuff.to_vec();
    Ok(ret)
}

#[wasm_bindgen(module = "/public/worker.js")]
extern "C" {
    #[wasm_bindgen(js_name = "isWorker")]
    pub fn is_worker() -> bool;
}

pub mod console {
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(js_namespace = console)]
        pub fn log(s: &str);
    }
}
