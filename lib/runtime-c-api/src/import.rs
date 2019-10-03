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
use std::{ffi::c_void, ptr, slice, sync::Arc};
use wasmer_runtime::{Global, Memory, Module, Table};
use wasmer_runtime_core::{
    export::{Context, Export, FuncPointer},
    import::ImportObject,
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

/// Creates a new empty import object.
/// See also `wasmer_import_object_append`
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub unsafe extern "C" fn wasmer_import_object_new() -> *mut wasmer_import_object_t {
    let import_object = Box::new(ImportObject::new());

    Box::into_raw(import_object) as *mut wasmer_import_object_t
}

#[cfg(feature = "wasi")]
mod wasi {
    use super::*;
    use crate::get_slice_checked;
    use std::path::PathBuf;

    /// Opens a directory that's visible to the WASI module as `alias` but
    /// is backed by the host file at `host_file_path`
    #[repr(C)]
    pub struct wasmer_wasi_map_dir_entry_t {
        /// What the WASI module will see in its virtual root
        pub alias: wasmer_byte_array,
        /// The backing file that the WASI module will interact with via the alias
        pub host_file_path: wasmer_byte_array,
    }

    impl wasmer_wasi_map_dir_entry_t {
        /// Converts the data into owned, Rust types
        pub unsafe fn as_tuple(&self) -> Result<(String, PathBuf), std::str::Utf8Error> {
            let alias = self.alias.as_str()?.to_owned();
            let host_path = std::path::PathBuf::from(self.host_file_path.as_str()?);

            Ok((alias, host_path))
        }
    }

    /// Creates a WASI import object.
    ///
    /// This function treats null pointers as empty collections.
    /// For example, passing null for a string in `args`, will lead to a zero
    /// length argument in that position.
    #[no_mangle]
    pub unsafe extern "C" fn wasmer_wasi_generate_import_object(
        args: *const wasmer_byte_array,
        args_len: c_uint,
        envs: *const wasmer_byte_array,
        envs_len: c_uint,
        preopened_files: *const wasmer_byte_array,
        preopened_files_len: c_uint,
        mapped_dirs: *const wasmer_wasi_map_dir_entry_t,
        mapped_dirs_len: c_uint,
    ) -> *mut wasmer_import_object_t {
        let arg_list = get_slice_checked(args, args_len as usize);
        let env_list = get_slice_checked(envs, envs_len as usize);
        let preopened_file_list = get_slice_checked(preopened_files, preopened_files_len as usize);
        let mapped_dir_list = get_slice_checked(mapped_dirs, mapped_dirs_len as usize);

        wasmer_wasi_generate_import_object_inner(
            arg_list,
            env_list,
            preopened_file_list,
            mapped_dir_list,
        )
        .unwrap_or(std::ptr::null_mut())
    }

    /// Inner function that wraps error handling
    fn wasmer_wasi_generate_import_object_inner(
        arg_list: &[wasmer_byte_array],
        env_list: &[wasmer_byte_array],
        preopened_file_list: &[wasmer_byte_array],
        mapped_dir_list: &[wasmer_wasi_map_dir_entry_t],
    ) -> Result<*mut wasmer_import_object_t, std::str::Utf8Error> {
        let arg_vec = arg_list.iter().map(|arg| unsafe { arg.as_vec() }).collect();
        let env_vec = env_list
            .iter()
            .map(|env_var| unsafe { env_var.as_vec() })
            .collect();
        let po_file_vec = preopened_file_list
            .iter()
            .map(|po_file| Ok(unsafe { po_file.as_str()? }.to_owned()))
            .collect::<Result<Vec<_>, _>>()?;
        let mapped_dir_vec = mapped_dir_list
            .iter()
            .map(|entry| unsafe { entry.as_tuple() })
            .collect::<Result<Vec<_>, _>>()?;

        let import_object = Box::new(wasmer_wasi::generate_import_object(
            arg_vec,
            env_vec,
            po_file_vec,
            mapped_dir_vec,
        ));
        Ok(Box::into_raw(import_object) as *mut wasmer_import_object_t)
    }

    /// Convenience function that creates a WASI import object with no arguments,
    /// environment variables, preopened files, or mapped directories.
    ///
    /// This function is the same as calling [`wasmer_wasi_generate_import_object`] with all
    /// empty values.
    #[no_mangle]
    pub unsafe extern "C" fn wasmer_wasi_generate_default_import_object(
    ) -> *mut wasmer_import_object_t {
        let import_object = Box::new(wasmer_wasi::generate_import_object(
            vec![],
            vec![],
            vec![],
            vec![],
        ));

        Box::into_raw(import_object) as *mut wasmer_import_object_t
    }
}

#[cfg(feature = "wasi")]
pub use self::wasi::*;

/// Extends an existing import object with new imports
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub unsafe extern "C" fn wasmer_import_object_extend(
    import_object: *mut wasmer_import_object_t,
    imports: *mut wasmer_import_t,
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
