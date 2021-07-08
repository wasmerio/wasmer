use super::engine::wasm_engine_t;
use wasmer_api::Store;

/// Opaque type representing a WebAssembly store.
#[allow(non_camel_case_types)]
pub struct wasm_store_t {
    pub(crate) inner: Store,
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
    let store = Store::new(&*engine.inner);

    Some(Box::new(wasm_store_t { inner: store }))
}

/// Deletes a WebAssembly store.
///
/// # Example
///
/// See the module's documentation.
#[no_mangle]
pub unsafe extern "C" fn wasm_store_delete(_store: Option<Box<wasm_store_t>>) {}
