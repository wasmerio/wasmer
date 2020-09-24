use super::store::wasm_store_t;
use super::{
    wasm_byte_vec_t, wasm_exporttype_t, wasm_exporttype_vec_t, wasm_importtype_t,
    wasm_importtype_vec_t,
};
use crate::c_try;
use std::mem;
use std::ptr::NonNull;
use std::slice;
use std::sync::Arc;
use wasmer::{Module, Store};

#[repr(C)]
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
    let mut exports = module
        .inner
        .exports()
        .map(Into::into)
        .map(Box::new)
        .map(Box::into_raw)
        .collect::<Vec<*mut wasm_exporttype_t>>();

    debug_assert_eq!(exports.len(), exports.capacity());
    out.size = exports.len();
    out.data = exports.as_mut_ptr();
    mem::forget(exports);
}

#[no_mangle]
pub unsafe extern "C" fn wasm_module_imports(
    module: &wasm_module_t,
    out: &mut wasm_importtype_vec_t,
) {
    let mut imports = module
        .inner
        .imports()
        .map(Into::into)
        .map(Box::new)
        .map(Box::into_raw)
        .collect::<Vec<*mut wasm_importtype_t>>();

    debug_assert_eq!(imports.len(), imports.capacity());
    out.size = imports.len();
    out.data = imports.as_mut_ptr();
    mem::forget(imports);
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
        Ok(mut byte_vec) => {
            byte_vec.shrink_to_fit();
            byte_vec
        }
        Err(_) => return,
    };
    // ensure we won't leak memory
    // TODO: use `Vec::into_raw_parts` when it becomes stable
    debug_assert_eq!(byte_vec.capacity(), byte_vec.len());
    out_ptr.size = byte_vec.len();
    out_ptr.data = byte_vec.as_mut_ptr();
    mem::forget(byte_vec);
}
