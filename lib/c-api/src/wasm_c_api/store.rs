use super::engine::wasm_engine_t;
use wasmer_api::{AsEngineRef, AsStoreMut, AsStoreRef, Store};

pub struct StoreRef {
    inner: Store,
}

impl Clone for StoreRef {
    fn clone(&self) -> Self {
        StoreRef {
            inner: self.inner.dangerous_clone(),
        }
    }
}

impl StoreRef {
    pub fn engine(&self) -> impl AsEngineRef + '_ {
        self.inner.engine()
    }

    pub unsafe fn store(&self) -> impl AsStoreRef + '_ {
        unsafe { self.inner.dangerous_ref_from_context() }
    }

    pub unsafe fn store_mut(&mut self) -> impl AsStoreMut + '_ {
        unsafe { self.inner.dangerous_mut_from_context() }
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_store_new(
    engine: Option<&wasm_engine_t>,
) -> Option<Box<wasm_store_t>> {
    let engine = engine?;
    let store = Store::new(engine.inner.clone());

    Some(Box::new(wasm_store_t {
        inner: StoreRef { inner: store },
    }))
}

/// Deletes a WebAssembly store.
///
/// # Example
///
/// See the module's documentation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_store_delete(_store: Option<Box<wasm_store_t>>) {}
