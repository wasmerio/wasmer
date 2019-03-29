//! Wasm module.

use crate::{
    error::{update_last_error, CApiError},
    export::wasmer_import_export_kind,
    import::wasmer_import_t,
    instance::wasmer_instance_t,
    wasmer_byte_array, wasmer_result_t,
};
use libc::{c_int, uint32_t, uint8_t};
use std::{collections::HashMap, slice};
use wasmer_runtime::{compile, default_compiler, Global, ImportObject, Memory, Module, Table};
use wasmer_runtime_core::{cache::Artifact, export::Export, import::Namespace, load_cache_with};

#[repr(C)]
pub struct wasmer_module_t;

#[repr(C)]
pub struct wasmer_serialized_module_t;

/// Creates a new Module from the given wasm bytes.
///
/// Returns `wasmer_result_t::WASMER_OK` upon success.
///
/// Returns `wasmer_result_t::WASMER_ERROR` upon failure. Use `wasmer_last_error_length`
/// and `wasmer_last_error_message` to get an error message.
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub unsafe extern "C" fn wasmer_compile(
    module: *mut *mut wasmer_module_t,
    wasm_bytes: *mut uint8_t,
    wasm_bytes_len: uint32_t,
) -> wasmer_result_t {
    let bytes: &[u8] = slice::from_raw_parts_mut(wasm_bytes, wasm_bytes_len as usize);
    let result = compile(bytes);
    let new_module = match result {
        Ok(instance) => instance,
        Err(error) => {
            update_last_error(error);
            return wasmer_result_t::WASMER_ERROR;
        }
    };
    *module = Box::into_raw(Box::new(new_module)) as *mut wasmer_module_t;
    wasmer_result_t::WASMER_OK
}

/// Returns true for valid wasm bytes and false for invalid bytes
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub unsafe extern "C" fn wasmer_validate(
    wasm_bytes: *const uint8_t,
    wasm_bytes_len: uint32_t,
) -> bool {
    if wasm_bytes.is_null() {
        return false;
    }
    let bytes: &[u8] = slice::from_raw_parts(wasm_bytes, wasm_bytes_len as usize);

    wasmer_runtime_core::validate(bytes)
}

/// Creates a new Instance from the given module and imports.
///
/// Returns `wasmer_result_t::WASMER_OK` upon success.
///
/// Returns `wasmer_result_t::WASMER_ERROR` upon failure. Use `wasmer_last_error_length`
/// and `wasmer_last_error_message` to get an error message.
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub unsafe extern "C" fn wasmer_module_instantiate(
    module: *const wasmer_module_t,
    instance: *mut *mut wasmer_instance_t,
    imports: *mut wasmer_import_t,
    imports_len: c_int,
) -> wasmer_result_t {
    let imports: &[wasmer_import_t] = slice::from_raw_parts(imports, imports_len as usize);
    let mut import_object = ImportObject::new();
    let mut namespaces = HashMap::new();
    for import in imports {
        let module_name = slice::from_raw_parts(
            import.module_name.bytes,
            import.module_name.bytes_len as usize,
        );
        let module_name = if let Ok(s) = std::str::from_utf8(module_name) {
            s
        } else {
            update_last_error(CApiError {
                msg: "error converting module name to string".to_string(),
            });
            return wasmer_result_t::WASMER_ERROR;
        };
        let import_name = slice::from_raw_parts(
            import.import_name.bytes,
            import.import_name.bytes_len as usize,
        );
        let import_name = if let Ok(s) = std::str::from_utf8(import_name) {
            s
        } else {
            update_last_error(CApiError {
                msg: "error converting import_name to string".to_string(),
            });
            return wasmer_result_t::WASMER_ERROR;
        };

        let namespace = namespaces.entry(module_name).or_insert_with(Namespace::new);

        let export = match import.tag {
            wasmer_import_export_kind::WASM_MEMORY => {
                let mem = import.value.memory as *mut Memory;
                Export::Memory((&*mem).clone())
            }
            wasmer_import_export_kind::WASM_FUNCTION => {
                let func_export = import.value.func as *mut Export;
                (&*func_export).clone()
            }
            wasmer_import_export_kind::WASM_GLOBAL => {
                let global = import.value.global as *mut Global;
                Export::Global((&*global).clone())
            }
            wasmer_import_export_kind::WASM_TABLE => {
                let table = import.value.table as *mut Table;
                Export::Table((&*table).clone())
            }
        };
        namespace.insert(import_name, export);
    }
    for (module_name, namespace) in namespaces.into_iter() {
        import_object.register(module_name, namespace);
    }

    let module = &*(module as *const Module);
    let new_instance = if let Ok(res) = module.instantiate(&import_object) {
        res
    } else {
        update_last_error(CApiError {
            msg: "error instantiating from module".to_string(),
        });
        return wasmer_result_t::WASMER_ERROR;
    };
    *instance = Box::into_raw(Box::new(new_instance)) as *mut wasmer_instance_t;
    wasmer_result_t::WASMER_OK
}

/// Serialize the given Module.
///
/// The caller owns the object and should call `wasmer_serialized_module_destroy` to free it.
///
/// Returns `wasmer_result_t::WASMER_OK` upon success.
///
/// Returns `wasmer_result_t::WASMER_ERROR` upon failure. Use `wasmer_last_error_length`
/// and `wasmer_last_error_message` to get an error message.
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub unsafe extern "C" fn wasmer_module_serialize(
    serialized_module: *mut *mut wasmer_serialized_module_t,
    module: *const wasmer_module_t,
) -> wasmer_result_t {
    let module = &*(module as *const Module);

    match module.cache() {
        Ok(artifact) => match artifact.serialize() {
            Ok(serialized_artifact) => {
                *serialized_module = Box::into_raw(Box::new(serialized_artifact)) as _;

                wasmer_result_t::WASMER_OK
            }
            Err(_) => {
                update_last_error(CApiError {
                    msg: "Failed to serialize the module artifact".to_string(),
                });
                wasmer_result_t::WASMER_ERROR
            }
        },
        Err(_) => {
            update_last_error(CApiError {
                msg: "Failed to serialize the module".to_string(),
            });
            wasmer_result_t::WASMER_ERROR
        }
    }
}

/// Get bytes of the serialized module.
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub unsafe extern "C" fn wasmer_serialized_module_bytes(
    serialized_module: *const wasmer_serialized_module_t,
) -> wasmer_byte_array {
    let serialized_module = &*(serialized_module as *const &[u8]);

    wasmer_byte_array {
        bytes: serialized_module.as_ptr(),
        bytes_len: serialized_module.len() as u32,
    }
}

/// Transform a sequence of bytes into a serialized module.
///
/// The caller owns the object and should call `wasmer_serialized_module_destroy` to free it.
///
/// Returns `wasmer_result_t::WASMER_OK` upon success.
///
/// Returns `wasmer_result_t::WASMER_ERROR` upon failure. Use `wasmer_last_error_length`
/// and `wasmer_last_error_message` to get an error message.
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub unsafe extern "C" fn wasmer_serialized_module_from_bytes(
    serialized_module: *mut *mut wasmer_serialized_module_t,
    serialized_module_bytes: *const uint8_t,
    serialized_module_bytes_length: uint32_t,
) -> wasmer_result_t {
    if serialized_module.is_null() {
        update_last_error(CApiError {
            msg: "`serialized_module_bytes` pointer is null".to_string(),
        });
        return wasmer_result_t::WASMER_ERROR;
    }

    let serialized_module_bytes: &[u8] = slice::from_raw_parts(
        serialized_module_bytes,
        serialized_module_bytes_length as usize,
    );

    *serialized_module = Box::into_raw(Box::new(serialized_module_bytes)) as _;
    wasmer_result_t::WASMER_OK
}

/// Deserialize the given serialized module.
///
/// Returns `wasmer_result_t::WASMER_OK` upon success.
///
/// Returns `wasmer_result_t::WASMER_ERROR` upon failure. Use `wasmer_last_error_length`
/// and `wasmer_last_error_message` to get an error message.
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub unsafe extern "C" fn wasmer_module_deserialize(
    module: *mut *mut wasmer_module_t,
    serialized_module: *const wasmer_serialized_module_t,
) -> wasmer_result_t {
    if serialized_module.is_null() {
        update_last_error(CApiError {
            msg: "`serialized_module` pointer is null".to_string(),
        });
        return wasmer_result_t::WASMER_ERROR;
    }

    let serialized_module: &[u8] = &*(serialized_module as *const &[u8]);

    match Artifact::deserialize(serialized_module) {
        Ok(artifact) => match load_cache_with(artifact, default_compiler()) {
            Ok(deserialized_module) => {
                *module = Box::into_raw(Box::new(deserialized_module)) as _;
                wasmer_result_t::WASMER_OK
            }
            Err(_) => {
                update_last_error(CApiError {
                    msg: "Failed to compile the serialized module".to_string(),
                });
                wasmer_result_t::WASMER_ERROR
            }
        },
        Err(_) => {
            update_last_error(CApiError {
                msg: "Failed to deserialize the module".to_string(),
            });
            wasmer_result_t::WASMER_ERROR
        }
    }
}

/// Frees memory for the given serialized Module.
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub extern "C" fn wasmer_serialized_module_destroy(
    serialized_module: *mut wasmer_serialized_module_t,
) {
    if !serialized_module.is_null() {
        unsafe { Box::from_raw(serialized_module as *mut &[u8]) };
    }
}

/// Frees memory for the given Module
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub extern "C" fn wasmer_module_destroy(module: *mut wasmer_module_t) {
    if !module.is_null() {
        unsafe { Box::from_raw(module as *mut Module) };
    }
}
