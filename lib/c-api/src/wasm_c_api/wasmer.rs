//! Wasmer-specific extensions to the Wasm C API.

use super::module::wasm_module_t;
use super::types::wasm_name_t;
use std::str;
use std::sync::Arc;

#[no_mangle]
pub unsafe extern "C" fn wasm_module_name(module: &wasm_module_t, out: &mut wasm_name_t) {
    let name = match module.inner.name() {
        Some(name) => name,
        None => return,
    };

    *out = name.as_bytes().to_vec().into();
}

#[no_mangle]
pub unsafe extern "C" fn wasm_module_set_name(
    module: &mut wasm_module_t,
    name: &wasm_name_t,
) -> bool {
    let name = match name.into_slice() {
        Some(name) => match str::from_utf8(name) {
            Ok(name) => name,
            Err(_) => return false, // not ideal!
        },
        None => return false,
    };

    match Arc::get_mut(&mut module.inner) {
        Some(module) => module.set_name(name),
        None => false,
    }
}
