//! Create, read, destroy import definitions (function, global, memory
//! and table) on an instance.

use crate::{
    error::{update_last_error, CApiError},
    export::{wasmer_import_export_kind, wasmer_import_export_value},
    module::wasmer_module_t,
    value::wasmer_value_tag,
    wasmer_byte_array, wasmer_result_t,
};
use libc::c_uint;
use std::{convert::TryFrom, ffi::c_void, ptr, slice, sync::Arc};
use wasmer_runtime::{Global, Memory, Module, Table};
use wasmer_runtime_core::{
    export::{Context, Export, FuncPointer},
    import::{ImportObject, ImportObjectIterator},
    module::ImportName,
    types::{FuncSig, Type},
};

#[repr(C)]
pub struct wasmer_import_t {
    pub module_name: wasmer_byte_array,
    pub import_name: wasmer_byte_array,
    pub tag: wasmer_import_export_kind,
    pub value: wasmer_import_export_value,
}

#[repr(C)]
pub struct wasmer_import_object_t;

#[repr(C)]
#[derive(Clone)]
pub struct wasmer_import_func_t;

#[repr(C)]
#[derive(Clone)]
pub struct wasmer_import_descriptor_t;

#[repr(C)]
#[derive(Clone)]
pub struct wasmer_import_descriptors_t;

#[repr(C)]
#[derive(Clone)]
pub struct wasmer_import_object_iter_t;

/// Creates a new empty import object.
/// See also `wasmer_import_object_append`
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub unsafe extern "C" fn wasmer_import_object_new() -> *mut wasmer_import_object_t {
    let import_object = Box::new(ImportObject::new());

    Box::into_raw(import_object) as *mut wasmer_import_object_t
}

#[cfg(feature = "wasi")]
mod wasi;

#[cfg(feature = "wasi")]
pub use self::wasi::*;

#[cfg(feature = "emscripten")]
mod emscripten;

#[cfg(feature = "emscripten")]
pub use self::emscripten::*;

/// Gets an entry from an ImportObject at the name and namespace.
/// Stores `name`, `namespace`, and `import_export_value` in `import`.
/// Thus these must remain valid for the lifetime of `import`.
///
/// The caller owns all data involved.
/// `import_export_value` will be written to based on `tag`.
#[no_mangle]
pub unsafe extern "C" fn wasmer_import_object_get_import(
    import_object: *const wasmer_import_object_t,
    namespace: wasmer_byte_array,
    name: wasmer_byte_array,
    import: *mut wasmer_import_t,
    import_export_value: *mut wasmer_import_export_value,
    tag: u32,
) -> wasmer_result_t {
    let tag: wasmer_import_export_kind = if let Ok(t) = TryFrom::try_from(tag) {
        t
    } else {
        update_last_error(CApiError {
            msg: "wasmer_import_export_tag out of range".to_string(),
        });
        return wasmer_result_t::WASMER_ERROR;
    };
    let import_object: &mut ImportObject = &mut *(import_object as *mut ImportObject);
    let namespace_str = if let Ok(ns) = namespace.as_str() {
        ns
    } else {
        update_last_error(CApiError {
            msg: "error converting namespace to UTF-8 string".to_string(),
        });
        return wasmer_result_t::WASMER_ERROR;
    };
    let name_str = if let Ok(name) = name.as_str() {
        name
    } else {
        update_last_error(CApiError {
            msg: "error converting name to UTF-8 string".to_string(),
        });
        return wasmer_result_t::WASMER_ERROR;
    };
    if import.is_null() || import_export_value.is_null() {
        update_last_error(CApiError {
            msg: "pointers to import and import_export_value must not be null".to_string(),
        });
        return wasmer_result_t::WASMER_ERROR;
    }
    let import_out = &mut *import;
    let import_export_value_out = &mut *import_export_value;
    if let Some(export) =
        import_object.maybe_with_namespace(namespace_str, |ns| ns.get_export(name_str))
    {
        match export {
            Export::Function { .. } => {
                if tag != wasmer_import_export_kind::WASM_FUNCTION {
                    update_last_error(CApiError {
                        msg: format!("Found function, expected {}", tag.to_str()),
                    });
                    return wasmer_result_t::WASMER_ERROR;
                }
                import_out.tag = wasmer_import_export_kind::WASM_FUNCTION;
                let writer = import_export_value_out.func as *mut Export;
                *writer = export.clone();
            }
            Export::Memory(memory) => {
                if tag != wasmer_import_export_kind::WASM_MEMORY {
                    update_last_error(CApiError {
                        msg: format!("Found memory, expected {}", tag.to_str()),
                    });
                    return wasmer_result_t::WASMER_ERROR;
                }
                import_out.tag = wasmer_import_export_kind::WASM_MEMORY;
                let writer = import_export_value_out.func as *mut Memory;
                *writer = memory.clone();
            }
            Export::Table(table) => {
                if tag != wasmer_import_export_kind::WASM_TABLE {
                    update_last_error(CApiError {
                        msg: format!("Found table, expected {}", tag.to_str()),
                    });
                    return wasmer_result_t::WASMER_ERROR;
                }
                import_out.tag = wasmer_import_export_kind::WASM_TABLE;
                let writer = import_export_value_out.func as *mut Table;
                *writer = table.clone();
            }
            Export::Global(global) => {
                if tag != wasmer_import_export_kind::WASM_GLOBAL {
                    update_last_error(CApiError {
                        msg: format!("Found global, expected {}", tag.to_str()),
                    });
                    return wasmer_result_t::WASMER_ERROR;
                }
                import_out.tag = wasmer_import_export_kind::WASM_GLOBAL;
                let writer = import_export_value_out.func as *mut Global;
                *writer = global.clone();
            }
        }

        import_out.value = *import_export_value;
        import_out.module_name = namespace;
        import_out.import_name = name;

        wasmer_result_t::WASMER_OK
    } else {
        update_last_error(CApiError {
            msg: format!("Export {} {} not found", namespace_str, name_str),
        });
        wasmer_result_t::WASMER_ERROR
    }
}

/// private wrapper data type used for casting
#[repr(C)]
struct WasmerImportObjectIterator(
    std::iter::Peekable<Box<dyn Iterator<Item = <ImportObjectIterator as Iterator>::Item>>>,
);

/// Create an iterator over the functions in the import object.
/// Get the next import with `wasmer_import_object_iter_next`
/// Free the iterator with `wasmer_import_object_iter_destroy`
#[no_mangle]
pub unsafe extern "C" fn wasmer_import_object_iterate_functions(
    import_object: *const wasmer_import_object_t,
) -> *mut wasmer_import_object_iter_t {
    if import_object.is_null() {
        update_last_error(CApiError {
            msg: "import_object must not be null".to_owned(),
        });
        return std::ptr::null_mut();
    }
    let import_object: &ImportObject = &*(import_object as *const ImportObject);
    let iter_inner = Box::new(import_object.clone_ref().into_iter().filter(|(_, _, e)| {
        if let Export::Function { .. } = e {
            true
        } else {
            false
        }
    })) as Box<dyn Iterator<Item = <ImportObjectIterator as Iterator>::Item>>;
    let iterator = Box::new(WasmerImportObjectIterator(iter_inner.peekable()));

    Box::into_raw(iterator) as *mut wasmer_import_object_iter_t
}

/// Writes the next value to `import`.  `WASMER_ERROR` is returned if there
/// was an error or there's nothing left to return.
///
/// To free the memory allocated here, pass the import to `wasmer_import_object_imports_destroy`.
/// To check if the iterator is done, use `wasmer_import_object_iter_at_end`.
#[no_mangle]
pub unsafe extern "C" fn wasmer_import_object_iter_next(
    import_object_iter: *mut wasmer_import_object_iter_t,
    import: *mut wasmer_import_t,
) -> wasmer_result_t {
    if import_object_iter.is_null() || import.is_null() {
        update_last_error(CApiError {
            msg: "import_object_iter and import must not be null".to_owned(),
        });
        return wasmer_result_t::WASMER_ERROR;
    }

    let iter = &mut *(import_object_iter as *mut WasmerImportObjectIterator);
    let out = &mut *import;
    // TODO: the copying here can be optimized away, we just need to use a different type of
    // iterator internally
    if let Some((namespace, name, export)) = iter.0.next() {
        let ns = {
            let mut n = namespace.clone();
            n.shrink_to_fit();
            n.into_bytes()
        };
        let ns_bytes = wasmer_byte_array {
            bytes: ns.as_ptr(),
            bytes_len: ns.len() as u32,
        };

        let name = {
            let mut n = name.clone();
            n.shrink_to_fit();
            n.into_bytes()
        };
        let name_bytes = wasmer_byte_array {
            bytes: name.as_ptr(),
            bytes_len: name.len() as u32,
        };

        out.module_name = ns_bytes;
        out.import_name = name_bytes;

        std::mem::forget(ns);
        std::mem::forget(name);

        match export {
            Export::Function { .. } => {
                let func = Box::new(export.clone());

                out.tag = wasmer_import_export_kind::WASM_FUNCTION;
                out.value = wasmer_import_export_value {
                    func: Box::into_raw(func) as *mut _ as *const _,
                };
            }
            Export::Global(global) => {
                let glbl = Box::new(global.clone());

                out.tag = wasmer_import_export_kind::WASM_GLOBAL;
                out.value = wasmer_import_export_value {
                    global: Box::into_raw(glbl) as *mut _ as *const _,
                };
            }
            Export::Memory(memory) => {
                let mem = Box::new(memory.clone());

                out.tag = wasmer_import_export_kind::WASM_MEMORY;
                out.value = wasmer_import_export_value {
                    memory: Box::into_raw(mem) as *mut _ as *const _,
                };
            }
            Export::Table(table) => {
                let tbl = Box::new(table.clone());

                out.tag = wasmer_import_export_kind::WASM_TABLE;
                out.value = wasmer_import_export_value {
                    memory: Box::into_raw(tbl) as *mut _ as *const _,
                };
            }
        }

        wasmer_result_t::WASMER_OK
    } else {
        wasmer_result_t::WASMER_ERROR
    }
}

/// Returns true if further calls to `wasmer_import_object_iter_next` will
/// not return any new data
#[no_mangle]
pub unsafe extern "C" fn wasmer_import_object_iter_at_end(
    import_object_iter: *mut wasmer_import_object_iter_t,
) -> bool {
    if import_object_iter.is_null() {
        update_last_error(CApiError {
            msg: "import_object_iter must not be null".to_owned(),
        });
        return true;
    }
    let iter = &mut *(import_object_iter as *mut WasmerImportObjectIterator);

    iter.0.peek().is_none()
}

/// Frees the memory allocated by `wasmer_import_object_iterate_functions`
#[no_mangle]
pub unsafe extern "C" fn wasmer_import_object_iter_destroy(
    import_object_iter: *mut wasmer_import_object_iter_t,
) {
    if !import_object_iter.is_null() {
        let _ = Box::from_raw(import_object_iter as *mut WasmerImportObjectIterator);
    }
}

/// Frees the memory allocated in `wasmer_import_object_iter_next`
///
/// This function does not free the memory in `wasmer_import_object_t`;
/// it only frees memory allocated while querying a `wasmer_import_object_t`.
#[no_mangle]
pub unsafe extern "C" fn wasmer_import_object_imports_destroy(
    imports: *mut wasmer_import_t,
    imports_len: u32,
) {
    if imports.is_null() {
        return;
    }
    let imports: &[wasmer_import_t] = &*slice::from_raw_parts_mut(imports, imports_len as usize);
    for import in imports {
        let _namespace: Vec<u8> = Vec::from_raw_parts(
            import.module_name.bytes as *mut u8,
            import.module_name.bytes_len as usize,
            import.module_name.bytes_len as usize,
        );
        let _name: Vec<u8> = Vec::from_raw_parts(
            import.import_name.bytes as *mut u8,
            import.import_name.bytes_len as usize,
            import.import_name.bytes_len as usize,
        );
        match import.tag {
            wasmer_import_export_kind::WASM_FUNCTION => {
                let _: Box<Export> = Box::from_raw(import.value.func as *mut _);
            }
            wasmer_import_export_kind::WASM_GLOBAL => {
                let _: Box<Global> = Box::from_raw(import.value.global as *mut _);
            }
            wasmer_import_export_kind::WASM_MEMORY => {
                let _: Box<Memory> = Box::from_raw(import.value.memory as *mut _);
            }
            wasmer_import_export_kind::WASM_TABLE => {
                let _: Box<Table> = Box::from_raw(import.value.table as *mut _);
            }
        }
    }
}

/// Extends an existing import object with new imports
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub unsafe extern "C" fn wasmer_import_object_extend(
    import_object: *mut wasmer_import_object_t,
    imports: *const wasmer_import_t,
    imports_len: c_uint,
) -> wasmer_result_t {
    let import_object: &mut ImportObject = &mut *(import_object as *mut ImportObject);

    let mut extensions: Vec<(String, String, Export)> = Vec::new();

    let imports: &[wasmer_import_t] = slice::from_raw_parts(imports, imports_len as usize);
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

        let extension = (module_name.to_string(), import_name.to_string(), export);
        extensions.push(extension)
    }

    import_object.extend(extensions);

    return wasmer_result_t::WASMER_OK;
}

/// Gets import descriptors for the given module
///
/// The caller owns the object and should call `wasmer_import_descriptors_destroy` to free it.
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub unsafe extern "C" fn wasmer_import_descriptors(
    module: *const wasmer_module_t,
    import_descriptors: *mut *mut wasmer_import_descriptors_t,
) {
    if module.is_null() {
        return;
    }
    let module = &*(module as *const Module);
    let total_imports = module.info().imported_functions.len()
        + module.info().imported_tables.len()
        + module.info().imported_globals.len()
        + module.info().imported_memories.len();
    let mut descriptors: Vec<NamedImportDescriptor> = Vec::with_capacity(total_imports);

    for (
        _index,
        ImportName {
            namespace_index,
            name_index,
        },
    ) in &module.info().imported_functions
    {
        let namespace = module.info().namespace_table.get(*namespace_index);
        let name = module.info().name_table.get(*name_index);
        descriptors.push(NamedImportDescriptor {
            module: namespace.to_string(),
            name: name.to_string(),
            kind: wasmer_import_export_kind::WASM_FUNCTION,
        });
    }

    for (
        _index,
        (
            ImportName {
                namespace_index,
                name_index,
            },
            _,
        ),
    ) in &module.info().imported_tables
    {
        let namespace = module.info().namespace_table.get(*namespace_index);
        let name = module.info().name_table.get(*name_index);
        descriptors.push(NamedImportDescriptor {
            module: namespace.to_string(),
            name: name.to_string(),
            kind: wasmer_import_export_kind::WASM_TABLE,
        });
    }

    for (
        _index,
        (
            ImportName {
                namespace_index,
                name_index,
            },
            _,
        ),
    ) in &module.info().imported_globals
    {
        let namespace = module.info().namespace_table.get(*namespace_index);
        let name = module.info().name_table.get(*name_index);
        descriptors.push(NamedImportDescriptor {
            module: namespace.to_string(),
            name: name.to_string(),
            kind: wasmer_import_export_kind::WASM_GLOBAL,
        });
    }

    for (
        _index,
        (
            ImportName {
                namespace_index,
                name_index,
            },
            _,
        ),
    ) in &module.info().imported_memories
    {
        let namespace = module.info().namespace_table.get(*namespace_index);
        let name = module.info().name_table.get(*name_index);
        descriptors.push(NamedImportDescriptor {
            module: namespace.to_string(),
            name: name.to_string(),
            kind: wasmer_import_export_kind::WASM_MEMORY,
        });
    }

    let named_import_descriptors: Box<NamedImportDescriptors> =
        Box::new(NamedImportDescriptors(descriptors));
    *import_descriptors =
        Box::into_raw(named_import_descriptors) as *mut wasmer_import_descriptors_t;
}

pub struct NamedImportDescriptors(Vec<NamedImportDescriptor>);

/// Frees the memory for the given import descriptors
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub extern "C" fn wasmer_import_descriptors_destroy(
    import_descriptors: *mut wasmer_import_descriptors_t,
) {
    if !import_descriptors.is_null() {
        unsafe { Box::from_raw(import_descriptors as *mut NamedImportDescriptors) };
    }
}

/// Gets the length of the import descriptors
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub unsafe extern "C" fn wasmer_import_descriptors_len(
    exports: *mut wasmer_import_descriptors_t,
) -> c_uint {
    if exports.is_null() {
        return 0;
    }
    (*(exports as *mut NamedImportDescriptors)).0.len() as c_uint
}

/// Gets import descriptor by index
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub unsafe extern "C" fn wasmer_import_descriptors_get(
    import_descriptors: *mut wasmer_import_descriptors_t,
    idx: c_uint,
) -> *mut wasmer_import_descriptor_t {
    if import_descriptors.is_null() {
        return ptr::null_mut();
    }
    let named_import_descriptors = &mut *(import_descriptors as *mut NamedImportDescriptors);
    &mut (*named_import_descriptors).0[idx as usize] as *mut NamedImportDescriptor
        as *mut wasmer_import_descriptor_t
}

/// Gets name for the import descriptor
#[no_mangle]
#[allow(clippy::cast_ptr_alignment)]
pub unsafe extern "C" fn wasmer_import_descriptor_name(
    import_descriptor: *mut wasmer_import_descriptor_t,
) -> wasmer_byte_array {
    let named_import_descriptor = &*(import_descriptor as *mut NamedImportDescriptor);
    wasmer_byte_array {
        bytes: named_import_descriptor.name.as_ptr(),
        bytes_len: named_import_descriptor.name.len() as u32,
    }
}

/// Gets module name for the import descriptor
#[no_mangle]
#[allow(clippy::cast_ptr_alignment)]
pub unsafe extern "C" fn wasmer_import_descriptor_module_name(
    import_descriptor: *mut wasmer_import_descriptor_t,
) -> wasmer_byte_array {
    let named_import_descriptor = &*(import_descriptor as *mut NamedImportDescriptor);
    wasmer_byte_array {
        bytes: named_import_descriptor.module.as_ptr(),
        bytes_len: named_import_descriptor.module.len() as u32,
    }
}

/// Gets export descriptor kind
#[no_mangle]
#[allow(clippy::cast_ptr_alignment)]
pub unsafe extern "C" fn wasmer_import_descriptor_kind(
    export: *mut wasmer_import_descriptor_t,
) -> wasmer_import_export_kind {
    let named_import_descriptor = &*(export as *mut NamedImportDescriptor);
    named_import_descriptor.kind.clone()
}

/// Sets the result parameter to the arity of the params of the wasmer_import_func_t
///
/// Returns `wasmer_result_t::WASMER_OK` upon success.
///
/// Returns `wasmer_result_t::WASMER_ERROR` upon failure. Use `wasmer_last_error_length`
/// and `wasmer_last_error_message` to get an error message.
#[no_mangle]
#[allow(clippy::cast_ptr_alignment)]
pub unsafe extern "C" fn wasmer_import_func_params_arity(
    func: *const wasmer_import_func_t,
    result: *mut u32,
) -> wasmer_result_t {
    let export = &*(func as *const Export);
    if let Export::Function { ref signature, .. } = *export {
        *result = signature.params().len() as u32;
        wasmer_result_t::WASMER_OK
    } else {
        update_last_error(CApiError {
            msg: "func ptr error in wasmer_import_func_params_arity".to_string(),
        });
        wasmer_result_t::WASMER_ERROR
    }
}

/// Creates new func
///
/// The caller owns the object and should call `wasmer_import_func_destroy` to free it.
#[no_mangle]
#[allow(clippy::cast_ptr_alignment)]
pub unsafe extern "C" fn wasmer_import_func_new(
    func: extern "C" fn(data: *mut c_void),
    params: *const wasmer_value_tag,
    params_len: c_uint,
    returns: *const wasmer_value_tag,
    returns_len: c_uint,
) -> *mut wasmer_import_func_t {
    let params: &[wasmer_value_tag] = slice::from_raw_parts(params, params_len as usize);
    let params: Vec<Type> = params.iter().cloned().map(|x| x.into()).collect();
    let returns: &[wasmer_value_tag] = slice::from_raw_parts(returns, returns_len as usize);
    let returns: Vec<Type> = returns.iter().cloned().map(|x| x.into()).collect();

    let export = Box::new(Export::Function {
        func: FuncPointer::new(func as _),
        ctx: Context::Internal,
        signature: Arc::new(FuncSig::new(params, returns)),
    });
    Box::into_raw(export) as *mut wasmer_import_func_t
}

/// Sets the params buffer to the parameter types of the given wasmer_import_func_t
///
/// Returns `wasmer_result_t::WASMER_OK` upon success.
///
/// Returns `wasmer_result_t::WASMER_ERROR` upon failure. Use `wasmer_last_error_length`
/// and `wasmer_last_error_message` to get an error message.
#[no_mangle]
#[allow(clippy::cast_ptr_alignment)]
pub unsafe extern "C" fn wasmer_import_func_params(
    func: *const wasmer_import_func_t,
    params: *mut wasmer_value_tag,
    params_len: c_uint,
) -> wasmer_result_t {
    let export = &*(func as *const Export);
    if let Export::Function { ref signature, .. } = *export {
        let params: &mut [wasmer_value_tag] =
            slice::from_raw_parts_mut(params, params_len as usize);
        for (i, item) in signature.params().iter().enumerate() {
            params[i] = item.into();
        }
        wasmer_result_t::WASMER_OK
    } else {
        update_last_error(CApiError {
            msg: "func ptr error in wasmer_import_func_params".to_string(),
        });
        wasmer_result_t::WASMER_ERROR
    }
}

/// Sets the returns buffer to the parameter types of the given wasmer_import_func_t
///
/// Returns `wasmer_result_t::WASMER_OK` upon success.
///
/// Returns `wasmer_result_t::WASMER_ERROR` upon failure. Use `wasmer_last_error_length`
/// and `wasmer_last_error_message` to get an error message.
#[no_mangle]
#[allow(clippy::cast_ptr_alignment)]
pub unsafe extern "C" fn wasmer_import_func_returns(
    func: *const wasmer_import_func_t,
    returns: *mut wasmer_value_tag,
    returns_len: c_uint,
) -> wasmer_result_t {
    let export = &*(func as *const Export);
    if let Export::Function { ref signature, .. } = *export {
        let returns: &mut [wasmer_value_tag] =
            slice::from_raw_parts_mut(returns, returns_len as usize);
        for (i, item) in signature.returns().iter().enumerate() {
            returns[i] = item.into();
        }
        wasmer_result_t::WASMER_OK
    } else {
        update_last_error(CApiError {
            msg: "func ptr error in wasmer_import_func_returns".to_string(),
        });
        wasmer_result_t::WASMER_ERROR
    }
}

/// Sets the result parameter to the arity of the returns of the wasmer_import_func_t
///
/// Returns `wasmer_result_t::WASMER_OK` upon success.
///
/// Returns `wasmer_result_t::WASMER_ERROR` upon failure. Use `wasmer_last_error_length`
/// and `wasmer_last_error_message` to get an error message.
#[no_mangle]
#[allow(clippy::cast_ptr_alignment)]
pub unsafe extern "C" fn wasmer_import_func_returns_arity(
    func: *const wasmer_import_func_t,
    result: *mut u32,
) -> wasmer_result_t {
    let export = &*(func as *const Export);
    if let Export::Function { ref signature, .. } = *export {
        *result = signature.returns().len() as u32;
        wasmer_result_t::WASMER_OK
    } else {
        update_last_error(CApiError {
            msg: "func ptr error in wasmer_import_func_results_arity".to_string(),
        });
        wasmer_result_t::WASMER_ERROR
    }
}

/// Frees memory for the given Func
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub extern "C" fn wasmer_import_func_destroy(func: *mut wasmer_import_func_t) {
    if !func.is_null() {
        unsafe { Box::from_raw(func as *mut Export) };
    }
}

/// Frees memory of the given ImportObject
#[no_mangle]
pub extern "C" fn wasmer_import_object_destroy(import_object: *mut wasmer_import_object_t) {
    if !import_object.is_null() {
        unsafe { Box::from_raw(import_object as *mut ImportObject) };
    }
}

struct NamedImportDescriptor {
    module: String,
    name: String,
    kind: wasmer_import_export_kind,
}
