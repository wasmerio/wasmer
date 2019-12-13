//! Functions and types for dealing with Emscripten imports

use super::*;
use crate::module::wasmer_module_t;
use std::ptr;
use wasmer_emscripten::EmscriptenGlobals;
use wasmer_runtime::Module;

/// Type used to construct an import_object_t with Emscripten imports.
#[repr(C)]
pub struct wasmer_emscripten_globals_t;

/// Create a `wasmer_emscripten_globals_t` from a Wasm module.
#[no_mangle]
pub unsafe extern "C" fn wasmer_emscripten_get_emscripten_globals(
    module: *const wasmer_module_t,
) -> *mut wasmer_emscripten_globals_t {
    if module.is_null() {
        return ptr::null_mut();
    }
    let module = &*(module as *const Module);
    match EmscriptenGlobals::new(module) {
        Ok(globals) => Box::into_raw(Box::new(globals)) as *mut wasmer_emscripten_globals_t,
        Err(msg) => {
            update_last_error(CApiError { msg });
            return ptr::null_mut();
        }
    }
}

/// Destroy `wasmer_emscrpten_globals_t` created by
/// `wasmer_emscripten_get_emscripten_globals`.
#[no_mangle]
pub unsafe extern "C" fn wasmer_emscripten_destroy_emscripten_globals(
    globals: *mut wasmer_emscripten_globals_t,
) {
    if globals.is_null() {
        return;
    }
    let _ = Box::from_raw(globals);
}

/// Create a `wasmer_import_object_t` with Emscripten imports, use
/// `wasmer_emscripten_get_emscripten_globals` to get a
/// `wasmer_emscripten_globals_t` from a `wasmer_module_t`.
#[no_mangle]
pub unsafe extern "C" fn wasmer_emscripten_generate_import_object(
    globals: *mut wasmer_emscripten_globals_t,
) -> *mut wasmer_import_object_t {
    if globals.is_null() {
        return ptr::null_mut();
    }
    // TODO: figure out if we should be using UnsafeCell here or something
    let g = &mut *(globals as *mut EmscriptenGlobals);
    let import_object = Box::new(wasmer_emscripten::generate_emscripten_env(g));

    Box::into_raw(import_object) as *mut wasmer_import_object_t
}
