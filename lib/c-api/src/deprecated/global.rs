//! Create, set, get and destroy global variables of an instance.

use crate::deprecated::{
    get_global_store,
    value::{wasmer_value_t, wasmer_value_tag},
};
use crate::error::update_last_error;
use std::ptr::NonNull;
use wasmer::Global;

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
pub extern "C" fn wasmer_global_new(value: wasmer_value_t, mutable: bool) -> *mut wasmer_global_t {
    let store = get_global_store();
    let global = if mutable {
        Global::new_mut(store, value.into())
    } else {
        Global::new(store, value.into())
    };
    Box::into_raw(Box::new(global)) as *mut wasmer_global_t
}

/// Gets the value stored by the given Global
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub unsafe extern "C" fn wasmer_global_get(global: *mut wasmer_global_t) -> wasmer_value_t {
    let global = &*(global as *mut Global);
    let value: wasmer_value_t = global.get().into();
    value
}

/// Sets the value stored by the given Global
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub unsafe extern "C" fn wasmer_global_set(global: *mut wasmer_global_t, value: wasmer_value_t) {
    let global = &*(global as *mut Global);
    if let Err(err) = global.set(value.into()) {
        update_last_error(err);
        // can't return an error without breaking the API, probaly a safe change
        // return wasmer_result_t::WASMER_ERROR;
    }
}

/// Returns a descriptor (type, mutability) of the given Global
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub unsafe extern "C" fn wasmer_global_get_descriptor(
    global: *mut wasmer_global_t,
) -> wasmer_global_descriptor_t {
    let global = &*(global as *mut Global);
    let descriptor = global.ty();
    wasmer_global_descriptor_t {
        mutable: descriptor.mutability.into(),
        kind: descriptor.ty.into(),
    }
}

/// Frees memory for the given Global
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub unsafe extern "C" fn wasmer_global_destroy(global: Option<NonNull<wasmer_global_t>>) {
    if let Some(global_inner) = global {
        Box::from_raw(global_inner.cast::<Global>().as_ptr());
    }
}
