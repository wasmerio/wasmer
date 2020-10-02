use super::store::wasm_store_t;
use super::types::{
    wasm_byte_vec_t, wasm_exporttype_t, wasm_exporttype_vec_t, wasm_importtype_t,
    wasm_importtype_vec_t,
};
use std::mem;
use std::ptr::NonNull;
use std::slice;
use std::sync::Arc;
use wasmer::{Module, Store};

#[allow(non_camel_case_types)]
pub struct wasm_module_t {
    pub(crate) inner: Arc<Module>,
}

#[no_mangle]
pub unsafe extern "C" fn wasm_module_new(
    store_ptr: Option<NonNull<wasm_store_t>>,
    bytes: &wasm_byte_vec_t,
) -> Option<Box<wasm_module_t>> {
    // TODO: review lifetime of byte slice
    let wasm_byte_slice: &[u8] = slice::from_raw_parts_mut(bytes.data, bytes.size);
    let store_ptr: NonNull<Store> = store_ptr?.cast::<Store>();
    let store = store_ptr.as_ref();
    let module = c_try!(Module::from_binary(store, wasm_byte_slice));

    Some(Box::new(wasm_module_t {
        inner: Arc::new(module),
    }))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_module_delete(_module: Option<Box<wasm_module_t>>) {}

#[no_mangle]
pub unsafe extern "C" fn wasm_module_exports(
    module: &wasm_module_t,
    out: &mut wasm_exporttype_vec_t,
) {
    let exports = module
        .inner
        .exports()
        .map(Into::into)
        .map(Box::new)
        .collect::<Vec<Box<wasm_exporttype_t>>>();

    *out = exports.into();
}

#[no_mangle]
pub unsafe extern "C" fn wasm_module_imports(
    module: &wasm_module_t,
    out: &mut wasm_importtype_vec_t,
) {
    let imports = module
        .inner
        .imports()
        .map(Into::into)
        .map(Box::new)
        .collect::<Vec<Box<wasm_importtype_t>>>();

    *out = imports.into();
}

#[no_mangle]
pub unsafe extern "C" fn wasm_module_deserialize(
    store_ptr: Option<NonNull<wasm_store_t>>,
    bytes: *const wasm_byte_vec_t,
) -> Option<NonNull<wasm_module_t>> {
    // TODO: read config from store and use that to decide which compiler to use

    let byte_slice = if bytes.is_null() || (&*bytes).into_slice().is_none() {
        // TODO: error handling here
        return None;
    } else {
        (&*bytes).into_slice().unwrap()
    };

    let store_ptr: NonNull<Store> = store_ptr?.cast::<Store>();
    let store = store_ptr.as_ref();
    let module = c_try!(Module::deserialize(store, byte_slice));

    Some(NonNull::new_unchecked(Box::into_raw(Box::new(
        wasm_module_t {
            inner: Arc::new(module),
        },
    ))))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_module_serialize(
    module: &wasm_module_t,
    out_ptr: &mut wasm_byte_vec_t,
) {
    let mut byte_vec = match module.inner.serialize() {
        Ok(byte_vec) => byte_vec,
        Err(_) => return,
    };
    *out_ptr = byte_vec.into();
}
