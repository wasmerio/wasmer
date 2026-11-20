//! What JavaScript functions keep alive.
//!
//! A JavaScript function that stands for a Rust one, such as a host function
//! of plain numbers, can own Rust values that it needs for as long as it is
//! called. They are dropped once the function is garbage collected, through
//! one `FinalizationRegistry` that every such function shares.

use js_sys::Function as JsFunction;
use wasm_bindgen::{JsCast, prelude::*};

#[wasm_bindgen]
extern "C" {
    /// A `FinalizationRegistry` that calls back with the number a function
    /// was registered with, once the function is garbage collected.
    #[wasm_bindgen(js_name = FinalizationRegistry)]
    type Finalizers;

    /// Fails where there is no `FinalizationRegistry`.
    #[wasm_bindgen(constructor, js_class = "FinalizationRegistry", catch)]
    fn new(cleanup: &JsFunction) -> Result<Finalizers, JsValue>;

    #[wasm_bindgen(method)]
    fn register(this: &Finalizers, function: &JsFunction, kept: f64);
}

/// A value that a JavaScript function keeps alive.
#[repr(C)]
struct Kept<T> {
    /// Drops the `Kept<T>` it is the first field of.
    drop: unsafe fn(*mut ()),
    value: T,
}

/// # Safety
///
/// `kept` must come from `Box::into_raw` of a `Kept<T>`, and is freed.
unsafe fn drop_kept<T>(kept: *mut ()) {
    drop(unsafe { Box::from_raw(kept.cast::<Kept<T>>()) });
}

thread_local! {
    /// Drops what each function keeps alive once the function is collected:
    /// one registry that every function shares, which lives as long as the
    /// thread. `None` where there is no `FinalizationRegistry`: what the
    /// functions keep alive then lives forever.
    static FINALIZERS: Option<Finalizers> = {
        let cleanup = Closure::<dyn Fn(f64)>::new(|kept: f64| {
            let kept = kept as usize as *mut ();
            // SAFETY: `keep_alive` registered the address of a `Kept<T>`,
            // whose first field drops it, and each registration calls back
            // at most once.
            unsafe { (*kept.cast::<unsafe fn(*mut ())>())(kept) }
        });
        let finalizers = Finalizers::new(cleanup.as_ref().unchecked_ref()).ok();
        // Called back for as long as the registry lives.
        cleanup.forget();
        finalizers
    };
}

/// Makes `function` keep `owned` alive for as long as it lives.
pub(crate) fn keep_alive<T: 'static>(owned: T, function: &JsFunction) {
    FINALIZERS.with(|finalizers| match finalizers {
        Some(finalizers) => {
            let kept = Box::into_raw(Box::new(Kept {
                drop: drop_kept::<T>,
                value: owned,
            }));
            finalizers.register(function, kept as usize as f64);
        }
        None => std::mem::forget(owned),
    })
}
