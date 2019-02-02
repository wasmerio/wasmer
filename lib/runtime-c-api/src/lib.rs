extern crate wasmer_runtime;

use std::os::raw::c_char;

use wasmer_runtime::ImportObject;

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

#[no_mangle]
pub extern "C" fn wasmer_import_object_new() -> *mut wasmer_import_object_t {
    Box::into_raw(Box::new(ImportObject::new())) as *mut wasmer_import_object_t
}

#[no_mangle]
pub extern "C" fn wasmer_import_object_destroy(import_object: *mut wasmer_import_object_t) {
    if !import_object.is_null() {
        drop(unsafe { Box::from_raw(import_object as *mut ImportObject) });
    }
}

#[no_mangle]
pub extern "C" fn wasmer_instantiate(
    mut instance: *mut wasmer_instance_t,
    bytes: *const c_char,
    import_object: *mut wasmer_import_object_t,
) -> wasmer_compile_result_t {
    let import_object = unsafe { Box::from_raw(import_object as *mut ImportObject) };
    let bytes = &[];
    let result = wasmer_runtime::instantiate(bytes, *import_object);
    let new_instance = match result {
        Ok(instance) => instance,
        Err(error) => return wasmer_compile_result_t::WASMER_COMPILE_ERROR,
    };
    instance = Box::into_raw(Box::new(new_instance)) as *mut wasmer_instance_t;
    wasmer_compile_result_t::WASMER_COMPILE_OK
}
