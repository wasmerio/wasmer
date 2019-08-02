// Instantiate a module, call functions, and read exports.

use crate::{
    error::{update_last_error, CApiError},
    export::{wasmer_exports_t, NamedExport, NamedExports},
    import::{
        wasmer_import_object_extend, wasmer_import_object_new, wasmer_import_object_t,
        wasmer_import_t,
    },
    memory::wasmer_memory_t,
    module::wasmer_module_t,
    value::{wasmer_value, wasmer_value_t, wasmer_value_tag},
    wasmer_result_t,
};
use libc::{c_char, c_uint, c_void};
use std::{ffi::CStr, slice};
use wasmer_runtime::{Ctx, Instance, Memory, Module, Value};
use wasmer_runtime_core::import::ImportObject;

#[repr(C)]
pub struct wasmer_instance_t;

#[repr(C)]
pub struct wasmer_instance_context_t;

/// Creates a new Instance from the given wasm bytes and imports.
///
/// Returns `wasmer_result_t::WASMER_OK` upon success.
///
/// Returns `wasmer_result_t::WASMER_ERROR` upon failure. Use `wasmer_last_error_length`
/// and `wasmer_last_error_message` to get an error message.
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub unsafe extern "C" fn wasmer_instantiate(
    instance: *mut *mut wasmer_instance_t,
    wasm_bytes: *mut u8,
    wasm_bytes_len: u32,
    imports: *mut wasmer_import_t,
    imports_len: c_uint,
) -> wasmer_result_t {
    if wasm_bytes.is_null() {
        update_last_error(CApiError {
            msg: "wasm bytes ptr is null".to_string(),
        });
        return wasmer_result_t::WASMER_ERROR;
    }

    let raw_import_object = wasmer_import_object_new();
    wasmer_import_object_extend(raw_import_object, imports, imports_len);

    let import_object: &mut ImportObject = &mut *(raw_import_object as *mut ImportObject);

    let bytes: &[u8] = slice::from_raw_parts_mut(wasm_bytes, wasm_bytes_len as usize);
    let result = wasmer_runtime::instantiate(bytes, &import_object);
    let new_instance = match result {
        Ok(instance) => instance,
        Err(error) => {
            update_last_error(error);
            return wasmer_result_t::WASMER_ERROR;
        }
    };
    *instance = Box::into_raw(Box::new(new_instance)) as *mut wasmer_instance_t;
    wasmer_result_t::WASMER_OK
}

/// Given:
/// * A prepared `wasmer` import-object
/// * A compiled wasmer module
///
/// Instantiates a wasmer instance
#[no_mangle]
pub unsafe extern "C" fn wasmer_module_import_instantiate(
    instance: *mut *mut wasmer_instance_t,
    module: *const wasmer_module_t,
    import_object: *const wasmer_import_object_t,
) -> wasmer_result_t {
    let import_object: &ImportObject = &*(import_object as *const ImportObject);
    let module: &Module = &*(module as *const Module);

    let new_instance: Instance = match module.instantiate(import_object) {
        Ok(instance) => instance,
        Err(error) => {
            update_last_error(error);
            return wasmer_result_t::WASMER_ERROR;
        }
    };
    *instance = Box::into_raw(Box::new(new_instance)) as *mut wasmer_instance_t;

    return wasmer_result_t::WASMER_OK;
}

/// Extracts the instance's context and returns it.
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub unsafe extern "C" fn wasmer_instance_context_get(
    instance: *mut wasmer_instance_t,
) -> *const wasmer_instance_context_t {
    let instance_ref = &*(instance as *const Instance);

    let ctx: *const Ctx = instance_ref.context() as *const _;

    ctx as *const wasmer_instance_context_t
}

/// Calls an instances exported function by `name` with the provided parameters.
/// Results are set using the provided `results` pointer.
///
/// Returns `wasmer_result_t::WASMER_OK` upon success.
///
/// Returns `wasmer_result_t::WASMER_ERROR` upon failure. Use `wasmer_last_error_length`
/// and `wasmer_last_error_message` to get an error message.
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub unsafe extern "C" fn wasmer_instance_call(
    instance: *mut wasmer_instance_t,
    name: *const c_char,
    params: *const wasmer_value_t,
    params_len: u32,
    results: *mut wasmer_value_t,
    results_len: u32,
) -> wasmer_result_t {
    if instance.is_null() {
        update_last_error(CApiError {
            msg: "instance ptr is null".to_string(),
        });
        return wasmer_result_t::WASMER_ERROR;
    }
    if name.is_null() {
        update_last_error(CApiError {
            msg: "name ptr is null".to_string(),
        });
        return wasmer_result_t::WASMER_ERROR;
    }
    if params.is_null() {
        update_last_error(CApiError {
            msg: "params ptr is null".to_string(),
        });
        return wasmer_result_t::WASMER_ERROR;
    }

    let params: &[wasmer_value_t] = slice::from_raw_parts(params, params_len as usize);
    let params: Vec<Value> = params.iter().cloned().map(|x| x.into()).collect();

    let func_name_c = CStr::from_ptr(name);
    let func_name_r = func_name_c.to_str().unwrap();

    let results: &mut [wasmer_value_t] = slice::from_raw_parts_mut(results, results_len as usize);
    let result = (&*(instance as *mut Instance)).call(func_name_r, &params[..]);

    match result {
        Ok(results_vec) => {
            if !results_vec.is_empty() {
                let ret = match results_vec[0] {
                    Value::I32(x) => wasmer_value_t {
                        tag: wasmer_value_tag::WASM_I32,
                        value: wasmer_value { I32: x },
                    },
                    Value::I64(x) => wasmer_value_t {
                        tag: wasmer_value_tag::WASM_I64,
                        value: wasmer_value { I64: x },
                    },
                    Value::F32(x) => wasmer_value_t {
                        tag: wasmer_value_tag::WASM_F32,
                        value: wasmer_value { F32: x },
                    },
                    Value::F64(x) => wasmer_value_t {
                        tag: wasmer_value_tag::WASM_F64,
                        value: wasmer_value { F64: x },
                    },
                    Value::V128(_) => unimplemented!("calling function with V128 parameter"),
                };
                results[0] = ret;
            }
            wasmer_result_t::WASMER_OK
        }
        Err(err) => {
            update_last_error(err);
            wasmer_result_t::WASMER_ERROR
        }
    }
}

/// Gets Exports for the given instance
///
/// The caller owns the object and should call `wasmer_exports_destroy` to free it.
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub unsafe extern "C" fn wasmer_instance_exports(
    instance: *mut wasmer_instance_t,
    exports: *mut *mut wasmer_exports_t,
) {
    let instance_ref = &mut *(instance as *mut Instance);
    let mut exports_vec: Vec<NamedExport> = Vec::with_capacity(instance_ref.exports().count());
    for (name, export) in instance_ref.exports() {
        exports_vec.push(NamedExport {
            name: name.clone(),
            export: export.clone(),
            instance: instance as *mut Instance,
        });
    }
    let named_exports: Box<NamedExports> = Box::new(NamedExports(exports_vec));
    *exports = Box::into_raw(named_exports) as *mut wasmer_exports_t;
}

/// Sets the `data` field of the instance context. This context will be
/// passed to all imported function for instance.
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub extern "C" fn wasmer_instance_context_data_set(
    instance: *mut wasmer_instance_t,
    data_ptr: *mut c_void,
) {
    let instance_ref = unsafe { &mut *(instance as *mut Instance) };
    instance_ref.context_mut().data = data_ptr;
}

/// Gets the memory within the context at the index `memory_idx`.
/// The index is always 0 until multiple memories are supported.
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub extern "C" fn wasmer_instance_context_memory(
    ctx: *const wasmer_instance_context_t,
    _memory_idx: u32,
) -> *const wasmer_memory_t {
    let ctx = unsafe { &*(ctx as *const Ctx) };
    let memory = ctx.memory(0);
    memory as *const Memory as *const wasmer_memory_t
}

/// Gets the `data` field within the context.
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub extern "C" fn wasmer_instance_context_data_get(
    ctx: *const wasmer_instance_context_t,
) -> *mut c_void {
    let ctx = unsafe { &*(ctx as *const Ctx) };
    ctx.data
}

/// Frees memory for the given Instance
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub extern "C" fn wasmer_instance_destroy(instance: *mut wasmer_instance_t) {
    if !instance.is_null() {
        unsafe { Box::from_raw(instance as *mut Instance) };
    }
}
