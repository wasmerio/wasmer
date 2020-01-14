//! Functions and types for dealing with Emscripten imports

use super::*;
use crate::{get_slice_checked, instance::wasmer_instance_t, module::wasmer_module_t};

use std::ptr;
use wasmer_emscripten::{EmscriptenData, EmscriptenGlobals};
use wasmer_runtime::{Instance, Module};

/// Type used to construct an import_object_t with Emscripten imports.
#[repr(C)]
pub struct wasmer_emscripten_globals_t;

/// Create a `wasmer_emscripten_globals_t` from a Wasm module.
#[no_mangle]
pub unsafe extern "C" fn wasmer_emscripten_get_globals(
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
pub unsafe extern "C" fn wasmer_emscripten_destroy_globals(
    globals: *mut wasmer_emscripten_globals_t,
) {
    if globals.is_null() {
        return;
    }
    let _ = Box::from_raw(globals);
}

/// Execute global constructors (required if the module is compiled from C++)
/// and sets up the internal environment.
///
/// This function sets the data pointer in the same way that
/// [`wasmer_instance_context_data_set`] does.
#[no_mangle]
pub unsafe extern "C" fn wasmer_emscripten_set_up(
    instance: *mut wasmer_instance_t,
    globals: *mut wasmer_emscripten_globals_t,
) -> wasmer_result_t {
    if globals.is_null() || instance.is_null() {
        return wasmer_result_t::WASMER_ERROR;
    }
    let instance = &mut *(instance as *mut Instance);
    let globals = &*(globals as *mut EmscriptenGlobals);
    let em_data = Box::into_raw(Box::new(EmscriptenData::new(
        instance,
        &globals.data,
        Default::default(),
    ))) as *mut c_void;
    instance.context_mut().data = em_data;

    match wasmer_emscripten::set_up_emscripten(instance) {
        Ok(_) => wasmer_result_t::WASMER_OK,
        Err(e) => {
            update_last_error(e);
            wasmer_result_t::WASMER_ERROR
        }
    }
}

/// Convenience function for setting up arguments and calling the Emscripten
/// main function.
///
/// WARNING:
///
/// Do not call this function on untrusted code when operating without
/// additional sandboxing in place.
/// Emscripten has access to many host system calls and therefore may do very
/// bad things.
#[no_mangle]
pub unsafe extern "C" fn wasmer_emscripten_call_main(
    instance: *mut wasmer_instance_t,
    args: *const wasmer_byte_array,
    args_len: c_uint,
) -> wasmer_result_t {
    if instance.is_null() || args.is_null() {
        return wasmer_result_t::WASMER_ERROR;
    }
    let instance = &mut *(instance as *mut Instance);

    let arg_list = get_slice_checked(args, args_len as usize);
    let arg_process_result: Result<Vec<&str>, _> =
        arg_list.iter().map(|arg| arg.as_str()).collect();
    let arg_vec = match arg_process_result.as_ref() {
        Ok(arg_vec) => arg_vec,
        Err(err) => {
            update_last_error(*err);
            return wasmer_result_t::WASMER_ERROR;
        }
    };

    let prog_name = if let Some(prog_name) = arg_vec.first() {
        prog_name
    } else {
        update_last_error(CApiError {
            msg: "First argument (program name) is required to execute Emscripten's main function"
                .to_string(),
        });
        return wasmer_result_t::WASMER_ERROR;
    };

    match wasmer_emscripten::emscripten_call_main(instance, prog_name, &arg_vec[1..]) {
        Ok(_) => wasmer_result_t::WASMER_OK,
        Err(e) => {
            update_last_error(e);
            wasmer_result_t::WASMER_ERROR
        }
    }
}

/// Create a `wasmer_import_object_t` with Emscripten imports, use
/// `wasmer_emscripten_get_emscripten_globals` to get a
/// `wasmer_emscripten_globals_t` from a `wasmer_module_t`.
///
/// WARNING:
///1
/// This `import_object_t` contains thin-wrappers around host system calls.
/// Do not use this to execute untrusted code without additional sandboxing.
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
