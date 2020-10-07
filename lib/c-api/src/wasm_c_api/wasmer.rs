//! Wasmer-specific extensions to the Wasm C API.

use crate::wasm_c_api::instance::wasm_instance_t;
use std::ffi::c_void;

#[no_mangle]
pub unsafe extern "C" fn wasm_instance_get_vmctx_ptr(instance: &wasm_instance_t) -> *mut c_void {
    instance.inner.vmctx_ptr() as _
}
