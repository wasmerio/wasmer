use super::engine::wasm_engine_t;
use wasmer::Store;

/// Opaque wrapper around `Store`
#[allow(non_camel_case_types)]
pub struct wasm_store_t {
    pub(crate) inner: Store,
}

#[no_mangle]
pub unsafe extern "C" fn wasm_store_new(
    engine: Option<&wasm_engine_t>,
) -> Option<Box<wasm_store_t>> {
    let engine = engine?;
    let store = Store::new(&*engine.inner);

    Some(Box::new(wasm_store_t { inner: store }))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_store_delete(_store: Option<Box<wasm_store_t>>) {}
