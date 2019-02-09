extern crate wasmer_runtime;
extern crate wasmer_runtime_core;

use libc::{c_char, c_int, int32_t, int64_t, uint32_t, uint8_t};
use std::ffi::CStr;
use std::slice;
use std::str;
use std::sync::Arc;
use std::{ffi::c_void, mem, ptr};
use wasmer_runtime::{ImportObject, Instance, Memory, Table, Value};
use wasmer_runtime_core::export::{Context, Export, FuncPointer};
use wasmer_runtime_core::import::{LikeNamespace, Namespace};
use wasmer_runtime_core::types::{ElementType, FuncSig, MemoryDescriptor, TableDescriptor, Type};
use wasmer_runtime_core::units::Pages;

#[allow(non_camel_case_types)]
pub struct wasmer_import_object_t();

#[allow(non_camel_case_types)]
pub struct wasmer_instance_t();

#[allow(non_camel_case_types)]
pub struct wasmer_instance_context_t();

#[allow(non_camel_case_types)]
#[no_mangle]
#[repr(C)]
pub enum wasmer_compile_result_t {
    WASMER_COMPILE_OK = 1,
    WASMER_COMPILE_ERROR = 2,
}

#[allow(non_camel_case_types)]
#[no_mangle]
#[repr(C)]
pub enum wasmer_call_result_t {
    WASMER_CALL_OK = 1,
    WASMER_CALL_ERROR = 2,
}

#[allow(non_camel_case_types)]
#[no_mangle]
#[repr(C)]
pub enum wasmer_memory_result_t {
    WASMER_MEMORY_OK = 1,
    WASMER_MEMORY_ERROR = 2,
}

#[allow(non_camel_case_types)]
#[no_mangle]
#[repr(C)]
pub enum wasmer_table_result_t {
    WASMER_TABLE_OK = 1,
    WASMER_TABLE_ERROR = 2,
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
pub struct wasmer_memory_t();

#[repr(C)]
#[derive(Clone)]
pub struct wasmer_table_t();

#[repr(C)]
pub struct wasmer_limits_t {
    pub min: uint32_t,
    pub max: uint32_t,
}

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

#[no_mangle]
pub extern "C" fn wasmer_import_object_new() -> *mut wasmer_import_object_t {
    Box::into_raw(Box::new(ImportObject::new())) as *mut wasmer_import_object_t
}

#[no_mangle]
pub unsafe extern "C" fn wasmer_memory_new(
    mut memory: *mut *mut wasmer_memory_t,
    limits: wasmer_limits_t,
) -> wasmer_memory_result_t {
    let desc = MemoryDescriptor {
        minimum: Pages(limits.min),
        maximum: Some(Pages(limits.max)),
        shared: false,
    };
    let result = Memory::new(desc);
    let new_memory = match result {
        Ok(memory) => memory,
        Err(error) => {
            return wasmer_memory_result_t::WASMER_MEMORY_ERROR;
        }
    };
    unsafe { *memory = Box::into_raw(Box::new(new_memory)) as *mut wasmer_memory_t };
    wasmer_memory_result_t::WASMER_MEMORY_OK
}

#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub extern "C" fn wasmer_memory_length(memory: *mut wasmer_memory_t) -> uint32_t {
    let memory = unsafe { Box::from_raw(memory as *mut Memory) };
    let Pages(len) = memory.size();
    Box::into_raw(memory);
    len
}

#[no_mangle]
pub unsafe extern "C" fn wasmer_table_new(
    mut table: *mut *mut wasmer_table_t,
    limits: wasmer_limits_t,
) -> wasmer_table_result_t {
    let desc = TableDescriptor {
        element: ElementType::Anyfunc,
        minimum: limits.min,
        maximum: Some(limits.max),
    };
    let result = Table::new(desc);
    let new_table = match result {
        Ok(table) => table,
        Err(error) => {
            return wasmer_table_result_t::WASMER_TABLE_ERROR;
        }
    };
    unsafe { *table = Box::into_raw(Box::new(new_table)) as *mut wasmer_table_t };
    wasmer_table_result_t::WASMER_TABLE_OK
}

#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub extern "C" fn wasmer_table_length(table: *mut wasmer_table_t) -> uint32_t {
    let table = unsafe { Box::from_raw(table as *mut Table) };
    let len = table.size();
    Box::into_raw(table);
    len
}

#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub extern "C" fn wasmer_table_destroy(table: *mut wasmer_table_t) {
    if !table.is_null() {
        drop(unsafe { Box::from_raw(table as *mut Table) });
    }
}

#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub extern "C" fn wasmer_import_object_destroy(import_object: *mut wasmer_import_object_t) {
    if !import_object.is_null() {
        drop(unsafe { Box::from_raw(import_object as *mut ImportObject) });
    }
}

#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub extern "C" fn wasmer_memory_destroy(memory: *mut wasmer_memory_t) {
    if !memory.is_null() {
        drop(unsafe { Box::from_raw(memory as *mut Memory) });
    }
}

#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub unsafe extern "C" fn wasmer_instantiate(
    mut instance: *mut *mut wasmer_instance_t,
    wasm_bytes: *mut uint8_t,
    wasm_bytes_len: uint32_t,
    import_object: *mut wasmer_import_object_t,
) -> wasmer_compile_result_t {
    let import_object = unsafe { Box::from_raw(import_object as *mut ImportObject) };
    if wasm_bytes.is_null() {
        return wasmer_compile_result_t::WASMER_COMPILE_ERROR;
    }
    let bytes: &[u8] =
        unsafe { ::std::slice::from_raw_parts_mut(wasm_bytes, wasm_bytes_len as usize) };
    let result = wasmer_runtime::instantiate(bytes, &*import_object);
    let new_instance = match result {
        Ok(instance) => instance,
        Err(error) => {
            return wasmer_compile_result_t::WASMER_COMPILE_ERROR;
        }
    };
    unsafe { *instance = Box::into_raw(Box::new(new_instance)) as *mut wasmer_instance_t };
    //    Box::into_raw(import_object); // TODO Review is this the correct way not to drop
    wasmer_compile_result_t::WASMER_COMPILE_OK
}

#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub unsafe extern "C" fn wasmer_instance_call(
    instance: *mut wasmer_instance_t,
    name: *const c_char,
    params: *const wasmer_value_t,
    params_len: c_int,
    results: *mut wasmer_value_t,
    results_len: c_int,
) -> wasmer_call_result_t {
    // TODO handle params and results
    if instance.is_null() {
        return wasmer_call_result_t::WASMER_CALL_ERROR;
    }
    if name.is_null() {
        return wasmer_call_result_t::WASMER_CALL_ERROR;
    }

    if params.is_null() {
        return wasmer_call_result_t::WASMER_CALL_ERROR;
    }

    let params: &[wasmer_value_t] = slice::from_raw_parts(params, params_len as usize);
    // TODO Fix this conversion and params
    let params: Vec<Value> = params.iter().cloned().map(|x| x.into()).collect();
    //    let params= &[Value::I32(3), Value::I32(4)];

    let func_name_c = unsafe { CStr::from_ptr(name) };
    let func_name_r = func_name_c.to_str().unwrap();
    let instance = unsafe { Box::from_raw(instance as *mut Instance) };

    let results: &mut [wasmer_value_t] = slice::from_raw_parts_mut(results, results_len as usize);
    let result = instance.call(func_name_r, &params[..]);
    Box::into_raw(instance);
    match result {
        Ok(results_vec) => {
            println!("Call Res: {:?}", results_vec);
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
            wasmer_call_result_t::WASMER_CALL_OK
        }
        Err(err) => {
            println!("Call Err: {:?}", err);
            wasmer_call_result_t::WASMER_CALL_ERROR
        }
    }
}

#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub unsafe extern "C" fn wasmer_imports_set_import_func(
    import_object: *mut wasmer_import_object_t,
    namespace: *const c_char,
    name: *const c_char,
    func: extern "C" fn(data: *mut c_void),
    params: *const wasmer_value_tag,
    params_len: c_int,
    returns: *const wasmer_value_tag,
    returns_len: c_int,
) {
    let mut import_object = unsafe { Box::from_raw(import_object as *mut ImportObject) };
    let namespace_c = unsafe { CStr::from_ptr(namespace) };
    let namespace_r = namespace_c.to_str().unwrap();
    let name_c = unsafe { CStr::from_ptr(name) };
    let name_r = name_c.to_str().unwrap();

    let params: &[wasmer_value_tag] = slice::from_raw_parts(params, params_len as usize);
    let params: Vec<Type> = params.iter().cloned().map(|x| x.into()).collect();
    let returns: &[wasmer_value_tag] = slice::from_raw_parts(returns, returns_len as usize);
    let returns: Vec<Type> = returns.iter().cloned().map(|x| x.into()).collect();

    let export = Export::Function {
        func: unsafe { FuncPointer::new(func as _) },
        ctx: Context::Internal,
        signature: Arc::new(FuncSig::new(params, returns)),
    };

    // TODO handle existing namespace
    //    let maybe_namespace = import_object.get_namespace(namespace_r);
    //    if let Some(n) = maybe_namespace {
    //        n.insert(name_r, export);
    //    } else {
    let mut namespace = Namespace::new();
    namespace.insert(name_r, export);
    import_object.register(namespace_r, namespace);
    Box::into_raw(import_object);
    //    };
}

//#[no_mangle]
//pub extern "C" fn wasmer_debug_print(kind: uint8_t, thing: *mut c_void) {
//    match kind {
//        1 => {
//            println!("wasmer import object:");
//            let import_object = unsafe { Box::from_raw(thing as *mut ImportObject) };
//            println!("after import object");
//            Box::into_raw(import_object);
//        },
//        _ => panic!("unknown kind {:?}", kind)
//    }
//}

#[no_mangle]
pub extern "C" fn wasmer_instance_context_memory(instance: *mut wasmer_instance_context_t) {}

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
