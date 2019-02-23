extern crate wasmer_runtime;
extern crate wasmer_runtime_core;

use libc::{c_char, c_int, int32_t, int64_t, uint32_t, uint8_t};
use std::cell::RefCell;
use std::collections::HashMap;
use std::error::Error;
use std::ffi::CStr;
use std::fmt;
use std::slice;
use std::sync::Arc;
use std::{ffi::c_void, ptr};
use wasmer_runtime::{Ctx, Global, ImportObject, Instance, Memory, Module, Table, Value};
use wasmer_runtime_core::export::{Context, Export, FuncPointer};
use wasmer_runtime_core::import::Namespace;
use wasmer_runtime_core::types::{ElementType, FuncSig, MemoryDescriptor, TableDescriptor, Type};
use wasmer_runtime_core::units::{Bytes, Pages};

#[allow(non_camel_case_types)]
pub struct wasmer_import_object_t();

#[allow(non_camel_case_types)]
pub struct wasmer_module_t();

#[allow(non_camel_case_types)]
pub struct wasmer_instance_t();

#[allow(non_camel_case_types)]
pub struct wasmer_instance_context_t();

#[allow(non_camel_case_types)]
#[no_mangle]
#[repr(C)]
pub enum wasmer_result_t {
    WASMER_OK = 1,
    WASMER_ERROR = 2,
}

#[repr(u32)]
#[derive(Clone)]
pub enum wasmer_value_tag {
    WASM_I32,
    WASM_I64,
    WASM_F32,
    WASM_F64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub union wasmer_value {
    I32: int32_t,
    I64: int64_t,
    F32: f32,
    F64: f64,
}

#[repr(C)]
#[derive(Clone)]
pub struct wasmer_value_t {
    tag: wasmer_value_tag,
    value: wasmer_value,
}

#[repr(C)]
#[derive(Clone)]
pub struct wasmer_global_descriptor_t {
    mutable: bool,
    kind: wasmer_value_tag,
}

#[repr(C)]
#[derive(Clone)]
pub struct wasmer_memory_t();

#[repr(C)]
#[derive(Clone)]
pub struct wasmer_table_t();

#[repr(C)]
#[derive(Clone)]
pub struct wasmer_func_t();

#[repr(C)]
#[derive(Clone)]
pub struct wasmer_global_t();

#[repr(C)]
pub struct wasmer_limits_t {
    pub min: uint32_t,
    pub max: wasmer_limit_option_t,
}

#[repr(C)]
pub struct wasmer_limit_option_t {
    pub has_some: bool,
    pub some: uint32_t,
}

#[repr(C)]
pub struct wasmer_func_signature {
    pub params: *const wasmer_value_tag,
    pub params_len: c_int,
    pub returns: *const wasmer_value_tag,
    pub returns_len: c_int,
}

#[repr(C)]
pub struct wasmer_import_t {
    module_name: wasmer_byte_array,
    import_name: wasmer_byte_array,
    tag: wasmer_import_export_kind,
    value: wasmer_import_export_value,
}

#[repr(C)]
#[derive(Clone)]
pub struct wasmer_export_t;

#[repr(C)]
#[derive(Clone)]
pub struct wasmer_exports_t;

#[repr(u32)]
#[derive(Clone)]
pub enum wasmer_import_export_kind {
    WASM_FUNCTION,
    WASM_GLOBAL,
    WASM_MEMORY,
    WASM_TABLE,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub union wasmer_import_export_value {
    func: *const wasmer_func_t,
    table: *const wasmer_table_t,
    memory: *const wasmer_memory_t,
    global: *const wasmer_global_t,
}

#[repr(C)]
pub struct wasmer_byte_array {
    bytes: *const uint8_t,
    bytes_len: uint32_t,
}

/// Returns true for valid wasm bytes and false for invalid bytes
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub unsafe extern "C" fn wasmer_validate(
    wasm_bytes: *mut uint8_t,
    wasm_bytes_len: uint32_t,
) -> bool {
    if wasm_bytes.is_null() {
        return false;
    }
    let bytes: &[u8] =
        unsafe { ::std::slice::from_raw_parts_mut(wasm_bytes, wasm_bytes_len as usize) };
    wasmer_runtime_core::validate(bytes)
}

/// Creates a new Memory for the given descriptor and initializes the given
/// pointer to pointer to a pointer to the new memory.
///
/// The caller owns the object and should call `wasmer_memory_destroy` to free it.
///
/// Returns `wasmer_result_t::WASMER_OK` upon success.
///
/// Returns `wasmer_result_t::WASMER_ERROR` upon failure. Use `wasmer_last_error_length`
/// and `wasmer_last_error_message` to get an error message.
#[no_mangle]
pub unsafe extern "C" fn wasmer_memory_new(
    mut memory: *mut *mut wasmer_memory_t,
    limits: wasmer_limits_t,
) -> wasmer_result_t {
    let max = if limits.max.has_some {
        Some(Pages(limits.max.some))
    } else {
        None
    };
    let desc = MemoryDescriptor {
        minimum: Pages(limits.min),
        maximum: max,
        shared: false,
    };
    let result = Memory::new(desc);
    let new_memory = match result {
        Ok(memory) => memory,
        Err(error) => {
            update_last_error(error);
            return wasmer_result_t::WASMER_ERROR;
        }
    };
    unsafe { *memory = Box::into_raw(Box::new(new_memory)) as *mut wasmer_memory_t };
    wasmer_result_t::WASMER_OK
}

/// Grows a Memory by the given number of pages.
///
/// Returns `wasmer_result_t::WASMER_OK` upon success.
///
/// Returns `wasmer_result_t::WASMER_ERROR` upon failure. Use `wasmer_last_error_length`
/// and `wasmer_last_error_message` to get an error message.
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub extern "C" fn wasmer_memory_grow(
    memory: *mut wasmer_memory_t,
    delta: uint32_t,
) -> wasmer_result_t {
    let memory = unsafe { &*(memory as *mut Memory) };
    let delta_result = memory.grow(Pages(delta));
    match delta_result {
        Ok(_) => wasmer_result_t::WASMER_OK,
        Err(grow_error) => {
            update_last_error(grow_error);
            wasmer_result_t::WASMER_ERROR
        }
    }
}

/// Returns the current length in pages of the given memory
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub extern "C" fn wasmer_memory_length(memory: *mut wasmer_memory_t) -> uint32_t {
    let memory = unsafe { &*(memory as *mut Memory) };
    let Pages(len) = memory.size();
    len
}

/// Creates a new Table for the given descriptor and initializes the given
/// pointer to pointer to a pointer to the new Table.
///
/// The caller owns the object and should call `wasmer_table_destroy` to free it.
///
/// Returns `wasmer_result_t::WASMER_OK` upon success.
///
/// Returns `wasmer_result_t::WASMER_ERROR` upon failure. Use `wasmer_last_error_length`
/// and `wasmer_last_error_message` to get an error message.
#[no_mangle]
pub unsafe extern "C" fn wasmer_table_new(
    mut table: *mut *mut wasmer_table_t,
    limits: wasmer_limits_t,
) -> wasmer_result_t {
    let max = if limits.max.has_some {
        Some(limits.max.some)
    } else {
        None
    };
    let desc = TableDescriptor {
        element: ElementType::Anyfunc,
        minimum: limits.min,
        maximum: max,
    };
    let result = Table::new(desc);
    let new_table = match result {
        Ok(table) => table,
        Err(error) => {
            update_last_error(error);
            return wasmer_result_t::WASMER_ERROR;
        }
    };
    unsafe { *table = Box::into_raw(Box::new(new_table)) as *mut wasmer_table_t };
    wasmer_result_t::WASMER_OK
}

/// Grows a Table by the given number of elements.
///
/// Returns `wasmer_result_t::WASMER_OK` upon success.
///
/// Returns `wasmer_result_t::WASMER_ERROR` upon failure. Use `wasmer_last_error_length`
/// and `wasmer_last_error_message` to get an error message.
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub extern "C" fn wasmer_table_grow(
    table: *mut wasmer_table_t,
    delta: uint32_t,
) -> wasmer_result_t {
    let table = unsafe { &*(table as *mut Table) };
    let delta_result = table.grow(delta);
    match delta_result {
        Ok(_) => wasmer_result_t::WASMER_OK,
        Err(grow_error) => {
            update_last_error(grow_error);
            wasmer_result_t::WASMER_ERROR
        }
    }
}

/// Returns the current length of the given Table
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub extern "C" fn wasmer_table_length(table: *mut wasmer_table_t) -> uint32_t {
    let table = unsafe { &*(table as *mut Table) };
    let len = table.size();
    len
}

/// Frees memory for the given Table
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub extern "C" fn wasmer_table_destroy(table: *mut wasmer_table_t) {
    if !table.is_null() {
        drop(unsafe { Box::from_raw(table as *mut Table) });
    }
}

/// Creates a new Global and returns a pointer to it.
/// The caller owns the object and should call `wasmer_global_destroy` to free it.
#[no_mangle]
pub unsafe extern "C" fn wasmer_global_new(
    value: wasmer_value_t,
    mutable: bool,
) -> *mut wasmer_global_t {
    let global = if mutable {
        Global::new_mutable(value.into())
    } else {
        Global::new(value.into())
    };
    unsafe { Box::into_raw(Box::new(global)) as *mut wasmer_global_t }
}

/// Gets the value stored by the given Global
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub extern "C" fn wasmer_global_get(global: *mut wasmer_global_t) -> wasmer_value_t {
    let global = unsafe { &*(global as *mut Global) };
    let value: wasmer_value_t = global.get().into();
    value
}

/// Sets the value stored by the given Global
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub extern "C" fn wasmer_global_set(global: *mut wasmer_global_t, value: wasmer_value_t) {
    let global = unsafe { &*(global as *mut Global) };
    global.set(value.into());
}

/// Returns a descriptor (type, mutability) of the given Global
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub extern "C" fn wasmer_global_get_descriptor(
    global: *mut wasmer_global_t,
) -> wasmer_global_descriptor_t {
    let global = unsafe { &*(global as *mut Global) };
    let descriptor = global.descriptor();
    let desc = wasmer_global_descriptor_t {
        mutable: descriptor.mutable,
        kind: descriptor.ty.into(),
    };
    desc
}

/// Frees memory for the given Global
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub extern "C" fn wasmer_global_destroy(global: *mut wasmer_global_t) {
    if !global.is_null() {
        drop(unsafe { Box::from_raw(global as *mut Global) });
    }
}

/// Frees memory for the given Memory
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub extern "C" fn wasmer_memory_destroy(memory: *mut wasmer_memory_t) {
    if !memory.is_null() {
        drop(unsafe { Box::from_raw(memory as *mut Memory) });
    }
}

/// Creates a new Module from the given wasm bytes.
///
/// Returns `wasmer_result_t::WASMER_OK` upon success.
///
/// Returns `wasmer_result_t::WASMER_ERROR` upon failure. Use `wasmer_last_error_length`
/// and `wasmer_last_error_message` to get an error message.
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub unsafe extern "C" fn wasmer_compile(
    mut module: *mut *mut wasmer_module_t,
    wasm_bytes: *mut uint8_t,
    wasm_bytes_len: uint32_t,
) -> wasmer_result_t {
    let bytes: &[u8] =
        unsafe { ::std::slice::from_raw_parts_mut(wasm_bytes, wasm_bytes_len as usize) };
    let result = wasmer_runtime::compile(bytes);
    let new_module = match result {
        Ok(instance) => instance,
        Err(error) => {
            update_last_error(error);
            return wasmer_result_t::WASMER_ERROR;
        }
    };
    unsafe { *module = Box::into_raw(Box::new(new_module)) as *mut wasmer_module_t };
    wasmer_result_t::WASMER_OK
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
    module: *mut wasmer_module_t,
    mut instance: *mut *mut wasmer_instance_t,
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

        let namespace = namespaces
            .entry(module_name)
            .or_insert_with(|| Namespace::new());

        let export = match import.tag {
            wasmer_import_export_kind::WASM_MEMORY => import.value.memory as *mut Export,
            wasmer_import_export_kind::WASM_FUNCTION => import.value.func as *mut Export,
            wasmer_import_export_kind::WASM_GLOBAL => import.value.global as *mut Export,
            wasmer_import_export_kind::WASM_TABLE => import.value.table as *mut Export,
        };
        namespace.insert(import_name, unsafe { (&*export).clone() });
    }
    for (module_name, namespace) in namespaces.into_iter() {
        import_object.register(module_name, namespace);
    }

    let module = unsafe { &*(module as *mut Module) };
    let new_instance = if let Ok(res) = module.instantiate(&import_object) {
        res
    } else {
        update_last_error(CApiError {
            msg: "error instantiating from module".to_string(),
        });
        return wasmer_result_t::WASMER_ERROR;
    };
    unsafe { *instance = Box::into_raw(Box::new(new_instance)) as *mut wasmer_instance_t };
    wasmer_result_t::WASMER_OK
}

/// Frees memory for the given Module
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub extern "C" fn wasmer_module_destroy(module: *mut wasmer_module_t) {
    if !module.is_null() {
        drop(unsafe { Box::from_raw(module as *mut Module) });
    }
}

/// Creates a new Instance from the given wasm bytes and imports.
///
/// Returns `wasmer_result_t::WASMER_OK` upon success.
///
/// Returns `wasmer_result_t::WASMER_ERROR` upon failure. Use `wasmer_last_error_length`
/// and `wasmer_last_error_message` to get an error message.
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub unsafe extern "C" fn wasmer_instantiate(
    mut instance: *mut *mut wasmer_instance_t,
    wasm_bytes: *mut uint8_t,
    wasm_bytes_len: uint32_t,
    imports: *mut wasmer_import_t,
    imports_len: c_int,
) -> wasmer_result_t {
    if wasm_bytes.is_null() {
        update_last_error(CApiError {
            msg: "wasm bytes ptr is null".to_string(),
        });
        return wasmer_result_t::WASMER_ERROR;
    }
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

        let namespace = namespaces
            .entry(module_name)
            .or_insert_with(|| Namespace::new());

        let export = match import.tag {
            wasmer_import_export_kind::WASM_MEMORY => import.value.memory as *mut Export,
            wasmer_import_export_kind::WASM_FUNCTION => import.value.func as *mut Export,
            wasmer_import_export_kind::WASM_GLOBAL => import.value.global as *mut Export,
            wasmer_import_export_kind::WASM_TABLE => import.value.table as *mut Export,
        };
        namespace.insert(import_name, unsafe { (&*export).clone() });
    }
    for (module_name, namespace) in namespaces.into_iter() {
        import_object.register(module_name, namespace);
    }

    let bytes: &[u8] =
        unsafe { ::std::slice::from_raw_parts_mut(wasm_bytes, wasm_bytes_len as usize) };
    let result = wasmer_runtime::instantiate(bytes, &import_object);
    let new_instance = match result {
        Ok(instance) => instance,
        Err(error) => {
            // TODO the trait bound `wasmer_runtime::error::Error: std::error::Error` is not satisfied
            //update_last_error(error);
            update_last_error(CApiError {
                msg: "error instantiating".to_string(),
            });
            return wasmer_result_t::WASMER_ERROR;
        }
    };
    unsafe { *instance = Box::into_raw(Box::new(new_instance)) as *mut wasmer_instance_t };
    wasmer_result_t::WASMER_OK
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
    params_len: c_int,
    results: *mut wasmer_value_t,
    results_len: c_int,
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

    let func_name_c = unsafe { CStr::from_ptr(name) };
    let func_name_r = func_name_c.to_str().unwrap();

    let results: &mut [wasmer_value_t] = slice::from_raw_parts_mut(results, results_len as usize);
    let result = (&*(instance as *mut Instance)).call(func_name_r, &params[..]);

    match result {
        Ok(results_vec) => {
            if results_vec.len() > 0 {
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
    let mut instance = unsafe { &mut *(instance as *mut Instance) };
    let named_exports: Box<NamedExports> =
        Box::new(NamedExports(instance.exports().map(|e| e.into()).collect()));
    unsafe { *exports = Box::into_raw(named_exports) as *mut wasmer_exports_t };
}

pub struct NamedExports(Vec<NamedExport>);

/// Frees the memory for the given exports
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub unsafe extern "C" fn wasmer_exports_destroy(exports: *mut wasmer_exports_t) {
    if !exports.is_null() {
        drop(unsafe { Box::from_raw(exports as *mut NamedExports) });
    }
}

/// Gets the length of the exports
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub unsafe extern "C" fn wasmer_exports_len(exports: *mut wasmer_exports_t) -> c_int {
    if exports.is_null() {
        return 0;
    }
    (*(exports as *mut NamedExports)).0.len() as c_int
}

/// Gets wasmer_export by index
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub unsafe extern "C" fn wasmer_exports_get(
    exports: *mut wasmer_exports_t,
    idx: c_int,
) -> *mut wasmer_export_t {
    if exports.is_null() {
        return ptr::null_mut();
    }
    let mut named_exports = unsafe { &mut *(exports as *mut NamedExports) };
    let ptr = &mut (*named_exports).0[idx as usize] as *mut NamedExport as *mut wasmer_export_t;
    ptr
}

/// Gets wasmer_export kind
#[no_mangle]
#[allow(clippy::cast_ptr_alignment)]
pub unsafe extern "C" fn wasmer_export_kind(
    export: *mut wasmer_export_t,
) -> wasmer_import_export_kind {
    let named_export = &*(export as *mut NamedExport);
    match named_export.export {
        Export::Table(_) => wasmer_import_export_kind::WASM_TABLE,
        Export::Function { .. } => wasmer_import_export_kind::WASM_FUNCTION,
        Export::Global(_) => wasmer_import_export_kind::WASM_GLOBAL,
        Export::Memory(_) => wasmer_import_export_kind::WASM_MEMORY,
    }
}

/// Creates new func
///
/// The caller owns the object and should call `wasmer_func_destroy` to free it.
#[no_mangle]
#[allow(clippy::cast_ptr_alignment)]
pub unsafe extern "C" fn wasmer_func_new(
    func: extern "C" fn(data: *mut c_void),
    params: *const wasmer_value_tag,
    params_len: c_int,
    returns: *const wasmer_value_tag,
    returns_len: c_int,
) -> *const wasmer_func_t {
    let params: &[wasmer_value_tag] = slice::from_raw_parts(params, params_len as usize);
    let params: Vec<Type> = params.iter().cloned().map(|x| x.into()).collect();
    let returns: &[wasmer_value_tag] = slice::from_raw_parts(returns, returns_len as usize);
    let returns: Vec<Type> = returns.iter().cloned().map(|x| x.into()).collect();

    let export = Box::new(Export::Function {
        func: unsafe { FuncPointer::new(func as _) },
        ctx: Context::Internal,
        signature: Arc::new(FuncSig::new(params, returns)),
    });
    Box::into_raw(export) as *mut wasmer_func_t
}

/// Sets the result parameter to the arity of the params of the wasmer_func_t
///
/// Returns `wasmer_result_t::WASMER_OK` upon success.
///
/// Returns `wasmer_result_t::WASMER_ERROR` upon failure. Use `wasmer_last_error_length`
/// and `wasmer_last_error_message` to get an error message.
#[no_mangle]
#[allow(clippy::cast_ptr_alignment)]
pub unsafe extern "C" fn wasmer_func_params_arity(
    func: *mut wasmer_func_t,
    result: *mut uint32_t,
) -> wasmer_result_t {
    let mut export = unsafe { &mut *(func as *mut Export) };
    let result = if let Export::Function { ref signature, .. } = *export {
        unsafe { *result = signature.params().len() as uint32_t };
        wasmer_result_t::WASMER_OK
    } else {
        update_last_error(CApiError {
            msg: "func ptr error in wasmer_func_params_arity".to_string(),
        });
        wasmer_result_t::WASMER_ERROR
    };
    result
}

/// Sets the params buffer to the parameter types of the given wasmer_func_t
///
/// Returns `wasmer_result_t::WASMER_OK` upon success.
///
/// Returns `wasmer_result_t::WASMER_ERROR` upon failure. Use `wasmer_last_error_length`
/// and `wasmer_last_error_message` to get an error message.
#[no_mangle]
#[allow(clippy::cast_ptr_alignment)]
pub unsafe extern "C" fn wasmer_func_params(
    func: *mut wasmer_func_t,
    params: *mut wasmer_value_tag,
    params_len: c_int,
) -> wasmer_result_t {
    let mut export = unsafe { &mut *(func as *mut Export) };
    let result = if let Export::Function { ref signature, .. } = *export {
        let params: &mut [wasmer_value_tag] =
            slice::from_raw_parts_mut(params, params_len as usize);
        for (i, item) in signature.params().iter().enumerate() {
            params[i] = item.into();
        }
        wasmer_result_t::WASMER_OK
    } else {
        update_last_error(CApiError {
            msg: "func ptr error in wasmer_func_params".to_string(),
        });
        wasmer_result_t::WASMER_ERROR
    };
    result
}

/// Sets the returns buffer to the parameter types of the given wasmer_func_t
///
/// Returns `wasmer_result_t::WASMER_OK` upon success.
///
/// Returns `wasmer_result_t::WASMER_ERROR` upon failure. Use `wasmer_last_error_length`
/// and `wasmer_last_error_message` to get an error message.
#[no_mangle]
#[allow(clippy::cast_ptr_alignment)]
pub unsafe extern "C" fn wasmer_func_returns(
    func: *mut wasmer_func_t,
    returns: *mut wasmer_value_tag,
    returns_len: c_int,
) -> wasmer_result_t {
    let mut export = unsafe { &mut *(func as *mut Export) };
    let result = if let Export::Function { ref signature, .. } = *export {
        let returns: &mut [wasmer_value_tag] =
            slice::from_raw_parts_mut(returns, returns_len as usize);
        for (i, item) in signature.returns().iter().enumerate() {
            returns[i] = item.into();
        }
        wasmer_result_t::WASMER_OK
    } else {
        update_last_error(CApiError {
            msg: "func ptr error in wasmer_func_returns".to_string(),
        });
        wasmer_result_t::WASMER_ERROR
    };
    result
}

/// Sets the result parameter to the arity of the returns of the wasmer_func_t
///
/// Returns `wasmer_result_t::WASMER_OK` upon success.
///
/// Returns `wasmer_result_t::WASMER_ERROR` upon failure. Use `wasmer_last_error_length`
/// and `wasmer_last_error_message` to get an error message.
#[no_mangle]
#[allow(clippy::cast_ptr_alignment)]
pub unsafe extern "C" fn wasmer_func_returns_arity(
    func: *mut wasmer_func_t,
    result: *mut uint32_t,
) -> wasmer_result_t {
    let mut export = unsafe { &*(func as *mut Export) };
    let result = if let Export::Function { ref signature, .. } = *export {
        unsafe { *result = signature.returns().len() as uint32_t };
        wasmer_result_t::WASMER_OK
    } else {
        update_last_error(CApiError {
            msg: "func ptr error in wasmer_func_results_arity".to_string(),
        });
        wasmer_result_t::WASMER_ERROR
    };
    result
}

/// Frees memory for the given Func
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub extern "C" fn wasmer_func_destroy(func: *mut wasmer_func_t) {
    if !func.is_null() {
        drop(unsafe { Box::from_raw(func as *mut Export) });
    }
}

/// Gets func from wasm_export
#[no_mangle]
#[allow(clippy::cast_ptr_alignment)]
pub unsafe extern "C" fn wasmer_export_to_func(
    export: *mut wasmer_export_t,
) -> *const wasmer_func_t {
    let named_export = &*(export as *mut NamedExport);
    &named_export.export as *const Export as *const wasmer_func_t
}

/// Gets name from wasmer_export
#[no_mangle]
#[allow(clippy::cast_ptr_alignment)]
pub unsafe extern "C" fn wasmer_export_name(export: *mut wasmer_export_t) -> wasmer_byte_array {
    let named_export = &*(export as *mut NamedExport);
    wasmer_byte_array {
        bytes: named_export.name.as_ptr(),
        bytes_len: named_export.name.len() as u32,
    }
}

/// Calls a `func` with the provided parameters.
/// Results are set using the provided `results` pointer.
///
/// Returns `wasmer_result_t::WASMER_OK` upon success.
///
/// Returns `wasmer_result_t::WASMER_ERROR` upon failure. Use `wasmer_last_error_length`
/// and `wasmer_last_error_message` to get an error message.
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub unsafe extern "C" fn wasmer_func_call(
    func: *mut wasmer_func_t,
    params: *const wasmer_value_t,
    params_len: c_int,
    results: *mut wasmer_value_t,
    results_len: c_int,
) -> wasmer_result_t {
    if func.is_null() {
        update_last_error(CApiError {
            msg: "func ptr is null".to_string(),
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

    let export_func = unsafe { &*(func as *mut Export) };

    let results: &mut [wasmer_value_t] = slice::from_raw_parts_mut(results, results_len as usize);
    //    TODO implement func.call
    update_last_error(CApiError {
        msg: "wasmer_func_call not yet implemented".to_string(),
    });
    wasmer_result_t::WASMER_ERROR
    //    let result = instance.call(func_name_r, &params[..]);
    //    Box::into_raw(export_func);
    //    match result {
    //        Ok(results_vec) => {
    //            if results_vec.len() > 0 {
    //                let ret = match results_vec[0] {
    //                    Value::I32(x) => wasmer_value_t {
    //                        tag: wasmer_value_tag::WASM_I32,
    //                        value: wasmer_value { I32: x },
    //                    },
    //                    Value::I64(x) => wasmer_value_t {
    //                        tag: wasmer_value_tag::WASM_I64,
    //                        value: wasmer_value { I64: x },
    //                    },
    //                    Value::F32(x) => wasmer_value_t {
    //                        tag: wasmer_value_tag::WASM_F32,
    //                        value: wasmer_value { F32: x },
    //                    },
    //                    Value::F64(x) => wasmer_value_t {
    //                        tag: wasmer_value_tag::WASM_F64,
    //                        value: wasmer_value { F64: x },
    //                    },
    //                };
    //                results[0] = ret;
    //            }
    //            wasmer_result_t::WASMER_OK
    //        }
    //        Err(err) => {
    //            update_last_error(err);
    //            wasmer_result_t::WASMER_ERROR
    //        }
    //    }
}

/// Gets the memory within the context at the index `memory_idx`.
/// The index is always 0 until multiple memories are supported.
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub extern "C" fn wasmer_instance_context_memory(
    ctx: *mut wasmer_instance_context_t,
    memory_idx: uint32_t,
) -> *const wasmer_memory_t {
    let mut ctx = unsafe { &mut *(ctx as *mut Ctx) };
    let memory = ctx.memory(0);
    memory as *const Memory as *const wasmer_memory_t
}

/// Gets the start pointer to the bytes within a Memory
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub extern "C" fn wasmer_memory_data(mem: *mut wasmer_memory_t) -> *mut uint8_t {
    let memory = mem as *mut Memory;
    use std::cell::Cell;
    unsafe { ((*memory).view::<u8>()[..]).as_ptr() as *mut Cell<u8> as *mut u8 }
}

/// Gets the size in bytes of a Memory
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub extern "C" fn wasmer_memory_data_length(mem: *mut wasmer_memory_t) -> uint32_t {
    let memory = mem as *mut Memory;
    let Bytes(len) = unsafe { (*memory).size().bytes() };
    len as uint32_t
}

/// Frees memory for the given Instance
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub extern "C" fn wasmer_instance_destroy(instance: *mut wasmer_instance_t) {
    if !instance.is_null() {
        drop(unsafe { Box::from_raw(instance as *mut Instance) });
    }
}

impl From<wasmer_value_t> for Value {
    fn from(v: wasmer_value_t) -> Self {
        unsafe {
            match v {
                wasmer_value_t {
                    tag: WASM_I32,
                    value: wasmer_value { I32 },
                } => Value::I32(I32),
                wasmer_value_t {
                    tag: WASM_I64,
                    value: wasmer_value { I64 },
                } => Value::I64(I64),
                wasmer_value_t {
                    tag: WASM_F32,
                    value: wasmer_value { F32 },
                } => Value::F32(F32),
                wasmer_value_t {
                    tag: WASM_F64,
                    value: wasmer_value { F64 },
                } => Value::F64(F64),
                _ => panic!("not implemented"),
            }
        }
    }
}

impl From<Value> for wasmer_value_t {
    fn from(val: Value) -> Self {
        match val {
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
        }
    }
}

impl From<Type> for wasmer_value_tag {
    fn from(ty: Type) -> Self {
        match ty {
            Type::I32 => wasmer_value_tag::WASM_I32,
            Type::I64 => wasmer_value_tag::WASM_I64,
            Type::F32 => wasmer_value_tag::WASM_F32,
            Type::F64 => wasmer_value_tag::WASM_F64,
            _ => panic!("not implemented"),
        }
    }
}

impl From<wasmer_value_tag> for Type {
    fn from(v: wasmer_value_tag) -> Self {
        unsafe {
            match v {
                wasmer_value_tag::WASM_I32 => Type::I32,
                wasmer_value_tag::WASM_I64 => Type::I64,
                wasmer_value_tag::WASM_F32 => Type::F32,
                wasmer_value_tag::WASM_F64 => Type::F64,
                _ => panic!("not implemented"),
            }
        }
    }
}

impl From<(std::string::String, wasmer_runtime_core::export::Export)> for NamedExport {
    fn from((name, export): (String, Export)) -> Self {
        NamedExport { name, export }
    }
}

impl From<&wasmer_runtime::wasm::Type> for wasmer_value_tag {
    fn from(ty: &Type) -> Self {
        match *ty {
            Type::I32 => wasmer_value_tag::WASM_I32,
            Type::I64 => wasmer_value_tag::WASM_I64,
            Type::F32 => wasmer_value_tag::WASM_F32,
            Type::F64 => wasmer_value_tag::WASM_F64,
        }
    }
}

// Error reporting

thread_local! {
    static LAST_ERROR: RefCell<Option<Box<Error>>> = RefCell::new(None);
}

fn update_last_error<E: Error + 'static>(err: E) {
    LAST_ERROR.with(|prev| {
        *prev.borrow_mut() = Some(Box::new(err));
    });
}

/// Retrieve the most recent error, clearing it in the process.
fn take_last_error() -> Option<Box<Error>> {
    LAST_ERROR.with(|prev| prev.borrow_mut().take())
}

/// Gets the length in bytes of the last error.
/// This can be used to dynamically allocate a buffer with the correct number of
/// bytes needed to store a message.
///
/// # Example
/// ```
/// int error_len = wasmer_last_error_length();
/// char *error_str = malloc(error_len);
/// ```
#[no_mangle]
pub extern "C" fn wasmer_last_error_length() -> c_int {
    LAST_ERROR.with(|prev| match *prev.borrow() {
        Some(ref err) => err.to_string().len() as c_int + 1,
        None => 0,
    })
}

/// Stores the last error message into the provided buffer up to the given `length`.
/// The `length` parameter must be large enough to store the last error message.
///
/// Returns the length of the string in bytes.
/// Returns `-1` if an error occurs.
///
/// # Example
/// ```
/// int error_len = wasmer_last_error_length();
/// char *error_str = malloc(error_len);
/// wasmer_last_error_message(error_str, error_len);
/// printf("Error str: `%s`\n", error_str);
/// ```
#[no_mangle]
pub unsafe extern "C" fn wasmer_last_error_message(buffer: *mut c_char, length: c_int) -> c_int {
    if buffer.is_null() {
        // buffer pointer is null
        return -1;
    }

    let last_error = match take_last_error() {
        Some(err) => err,
        None => return 0,
    };

    let error_message = last_error.to_string();

    let buffer = slice::from_raw_parts_mut(buffer as *mut u8, length as usize);

    if error_message.len() >= buffer.len() {
        // buffer to small for err  message
        return -1;
    }

    ptr::copy_nonoverlapping(
        error_message.as_ptr(),
        buffer.as_mut_ptr(),
        error_message.len(),
    );

    // Add a trailing null so people using the string as a `char *` don't
    // accidentally read into garbage.
    buffer[error_message.len()] = 0;

    error_message.len() as c_int
}

#[derive(Debug)]
struct CApiError {
    msg: String,
}

impl fmt::Display for CApiError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", &self.msg)
    }
}

impl Error for CApiError {}

struct NamedExport {
    name: String,
    export: Export,
}
