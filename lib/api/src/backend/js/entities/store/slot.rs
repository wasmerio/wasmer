//! How host functions reach their store.
//!
//! A host function on this backend is a JavaScript function. It lives as long
//! as anything holds it, such as a guest that imports it or the export of an
//! instance, and that can be longer than its store. So a host function does
//! not keep a pointer to its store but shares the store's slot, which holds
//! one: the store empties it when it is dropped, and a host function called
//! after that fails the call instead of using freed memory.

use std::{cell::Cell, ptr, rc::Rc};

use js_sys::Function as JsFunction;
use wasm_bindgen::{JsCast, JsValue, prelude::*};

use crate::{AsStoreRef, BackendStore, RuntimeError, StoreInner, StoreMut};

/// The slot of a store, shared with its host functions.
#[derive(Debug, Clone)]
pub(crate) struct StoreSlot(Rc<Cell<*mut StoreInner>>);

impl StoreSlot {
    /// An empty slot, until [`StoreSlot::bind`] fills it.
    pub(crate) fn new() -> Self {
        Self(Rc::new(Cell::new(ptr::null_mut())))
    }

    /// Points the slot of `inner` at it, once it has the address it keeps.
    pub(crate) fn bind(inner: &mut StoreInner) {
        let address: *mut StoreInner = inner;
        #[allow(irrefutable_let_patterns)]
        if let BackendStore::Js(store) = &inner.store {
            store.slot.0.set(address);
        }
    }

    /// The slot of `store`.
    pub(crate) fn of(store: &impl AsStoreRef) -> Self {
        store.as_store_ref().inner.store.as_js().slot.clone()
    }

    /// The store is dropped: its host functions cannot use it anymore.
    pub(crate) fn clear(&self) {
        self.0.set(ptr::null_mut());
    }

    /// The store, for a host function to use during its call. Fails if the
    /// store was dropped.
    ///
    /// # Safety
    ///
    /// Only for a host function of this slot's store, while it is called by a
    /// guest, and for no longer than that call.
    #[inline]
    pub(crate) unsafe fn store<'a>(&self) -> Result<StoreMut<'a>, JsValue> {
        unsafe { store_in(&self.0) }
    }

    /// What [`StoreSlot::store`] makes its store from.
    #[cfg(feature = "experimental-async")]
    pub(crate) fn inner(&self) -> Result<*mut StoreInner, JsValue> {
        match self.0.get() {
            inner if inner.is_null() => Err(dropped()),
            inner => Ok(inner),
        }
    }

    /// The address of the slot, which a host function that is not a Rust
    /// closure is given as a number. Such a function must keep the slot alive
    /// with [`StoreSlot::keep_alive_by`].
    pub(crate) fn address(&self) -> f64 {
        Rc::as_ptr(&self.0) as usize as f64
    }

    /// Makes `function` keep the slot alive for as long as it lives.
    pub(crate) fn keep_alive_by(&self, function: &JsFunction) {
        keep_alive(self.clone(), function);
    }
}

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

/// The store in the slot at `address`, which a host function was given by
/// [`StoreSlot::address`]. Fails if the store was dropped.
///
/// # Safety
///
/// As for [`StoreSlot::store`], and `address` must come from the slot of the
/// host function that is called.
#[inline]
pub(crate) unsafe fn store_at<'a>(address: usize) -> Result<StoreMut<'a>, JsValue> {
    unsafe { store_in(&*(address as *const Cell<*mut StoreInner>)) }
}

#[inline]
unsafe fn store_in<'a>(slot: &Cell<*mut StoreInner>) -> Result<StoreMut<'a>, JsValue> {
    let inner = slot.get();
    if inner.is_null() {
        return Err(dropped());
    }
    Ok(unsafe { StoreMut::from_raw(inner) })
}

#[cold]
fn dropped() -> JsValue {
    let error = RuntimeError::new("a host function was called after its store was dropped");
    crate::backend::js::error::Trap::user(Box::new(error)).into()
}
