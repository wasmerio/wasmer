use std::cell::Cell;

use js_sys::Function;
#[allow(unused_imports, dead_code)]
use tracing::{debug, error, info, trace, warn};
use wasm_bindgen::{prelude::*, JsCast};
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
