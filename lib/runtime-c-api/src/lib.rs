extern crate wasmer_runtime;

use libc::{c_char, c_int, int32_t, uint32_t, uint8_t};
use std::ffi::CStr;
use std::slice;
use std::str;
use wasmer_runtime::{ImportObject, Instance, Value};

#[allow(non_camel_case_types)]
pub struct wasmer_import_object_t();

#[allow(non_camel_case_types)]
pub struct wasmer_instance_t();

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

#[no_mangle]
pub extern "C" fn wasmer_import_object_new() -> *mut wasmer_import_object_t {
    Box::into_raw(Box::new(ImportObject::new())) as *mut wasmer_import_object_t
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
    let result = wasmer_runtime::instantiate(bytes, *import_object);
    let new_instance = match result {
        Ok(instance) => instance,
        Err(error) => {
            println!("Err: {:?}", error);
            return wasmer_compile_result_t::WASMER_COMPILE_ERROR;
        }
    };
    unsafe { *instance = Box::into_raw(Box::new(new_instance)) as *mut wasmer_instance_t };
    wasmer_compile_result_t::WASMER_COMPILE_OK
}

#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub unsafe extern "C" fn wasmer_instance_call(
    instance: *mut wasmer_instance_t,
    name: *const c_char,
    params: *const uint32_t,
    params_len: c_int,
    results: *mut uint32_t,
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

    let params: &[uint32_t] = slice::from_raw_parts(params, params_len as usize);
    // TODO Fix this conversion and params
    let params: Vec<Value> = params
        .iter()
        .cloned()
        .map(|x| Value::I32(x as i32))
        .collect();
    //    let params= &[Value::I32(3), Value::I32(4)];

    let func_name_c = unsafe { CStr::from_ptr(name) };
    let func_name_r = func_name_c.to_str().unwrap();
    let instance = unsafe { Box::from_raw(instance as *mut Instance) };

    let results: &mut [uint32_t] = slice::from_raw_parts_mut(results, results_len as usize);
    let result = instance.call(func_name_r, &params[..]);
    match result {
        Ok(res) => {
            println!("Res: {:?}", res);
            if let Value::I32(x) = res[0] {
                results[0] = x as u32;
            }
            wasmer_call_result_t::WASMER_CALL_OK
        }
        Err(err) => {
            println!("Err: {:?}", err);
            wasmer_call_result_t::WASMER_CALL_ERROR
        }
    }
}

#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub extern "C" fn wasmer_instance_destroy(instance: *mut wasmer_instance_t) {
    if !instance.is_null() {
        drop(unsafe { Box::from_raw(instance as *mut Instance) });
    }
}
