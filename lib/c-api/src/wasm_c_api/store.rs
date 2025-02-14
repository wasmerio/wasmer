use super::engine::wasm_engine_t;
use std::cell::UnsafeCell;
use std::rc::Rc;
use wasmer_api::{AsStoreMut, AsStoreRef, Store, StoreMut, StoreRef as BaseStoreRef};

#[derive(Clone)]
pub struct StoreRef {
    inner: Rc<UnsafeCell<Store>>,
}

impl StoreRef {
    pub unsafe fn store(&self) -> BaseStoreRef<'_> {
        (*self.inner.get()).as_store_ref()
    }

    pub unsafe fn store_mut(&mut self) -> StoreMut<'_> {
        (*self.inner.get()).as_store_mut()
    }
}

/// Opaque type representing a WebAssembly store.
#[allow(non_camel_case_types)]
pub struct wasm_store_t {
    pub(crate) inner: StoreRef,
}

/// Creates a new WebAssembly store given a specific [engine][super::engine].
///
/// # Example
///
/// See the module's documentation.
#[no_mangle]
pub unsafe extern "C" fn wasm_store_new(
    engine: Option<&wasm_engine_t>,
) -> Option<Box<wasm_store_t>> {
    let engine = engine?;
    let store = Store::new(engine.inner.clone());

    Some(Box::new(wasm_store_t {
        inner: StoreRef {
            inner: Rc::new(UnsafeCell::new(store)),
        },
    }))
}

/// Deletes a WebAssembly store.
///
/// # Example
///
/// See the module's documentation.
#[no_mangle]
pub unsafe extern "C" fn wasm_store_delete(_store: Option<Box<wasm_store_t>>) {}
