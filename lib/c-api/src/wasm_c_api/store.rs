use super::engine::wasm_engine_t;
use std::ptr::NonNull;
use wasmer::Store;

/// Opaque wrapper around `Store`
#[allow(non_camel_case_types)]
pub struct wasm_store_t {}

#[no_mangle]
pub unsafe extern "C" fn wasm_store_new(
    wasm_engine_ptr: Option<NonNull<wasm_engine_t>>,
) -> Option<NonNull<wasm_store_t>> {
    let wasm_engine_ptr = wasm_engine_ptr?;
    let wasm_engine = wasm_engine_ptr.as_ref();
    let store = Store::new(&*wasm_engine.inner);
    Some(NonNull::new_unchecked(
        Box::into_raw(Box::new(store)) as *mut wasm_store_t
    ))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_store_delete(wasm_store: Option<NonNull<wasm_store_t>>) {
    if let Some(s_inner) = wasm_store {
        // this should not leak memory:
        // we should double check it to make sure though
        let _: Box<Store> = Box::from_raw(s_inner.cast::<Store>().as_ptr());
    }
}
