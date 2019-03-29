//! Wasm global.

use crate::value::{wasmer_value_t, wasmer_value_tag};
use wasmer_runtime::Global;

#[repr(C)]
#[derive(Clone)]
pub struct wasmer_global_descriptor_t {
    mutable: bool,
    kind: wasmer_value_tag,
}

#[repr(C)]
#[derive(Clone)]
pub struct wasmer_global_t;

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
    Box::into_raw(Box::new(global)) as *mut wasmer_global_t
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
    wasmer_global_descriptor_t {
        mutable: descriptor.mutable,
        kind: descriptor.ty.into(),
    }
}

/// Frees memory for the given Global
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub extern "C" fn wasmer_global_destroy(global: *mut wasmer_global_t) {
    if !global.is_null() {
        unsafe { Box::from_raw(global as *mut Global) };
    }
}
