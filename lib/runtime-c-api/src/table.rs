//! Create, grow, destroy tables of an instance.

use crate::{error::update_last_error, wasmer_limits_t, wasmer_result_t};
use wasmer::{Table, Val, ValType};

#[repr(C)]
#[derive(Clone)]
pub struct wasmer_table_t;

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
    table: *mut *mut wasmer_table_t,
    limits: wasmer_limits_t,
) -> wasmer_result_t {
    unimplemented!("wasmer_table_new needs a global store")
    /*let max = if limits.max.has_some {
        Some(limits.max.some)
    } else {
        None
    };
    let desc = TableType {
        ty: Type::FuncRef,
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
    *table = Box::into_raw(Box::new(new_table)) as *mut wasmer_table_t;
    wasmer_result_t::WASMER_OK
    */
}

/// Grows a Table by the given number of elements.
///
/// Returns `wasmer_result_t::WASMER_OK` upon success.
///
/// Returns `wasmer_result_t::WASMER_ERROR` upon failure. Use `wasmer_last_error_length`
/// and `wasmer_last_error_message` to get an error message.
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub extern "C" fn wasmer_table_grow(table: *mut wasmer_table_t, delta: u32) -> wasmer_result_t {
    let table = unsafe { &*(table as *mut Table) };
    let table_type = table.ty().ty;
    // TODO: this logic should be in wasmer itself
    let table_default_value = match table_type {
        ValType::I32 => Val::I32(0),
        ValType::I64 => Val::I64(0),
        ValType::F32 => Val::F32(0.),
        ValType::F64 => Val::F64(0.),
        ValType::V128 => Val::V128(0),
        ValType::AnyRef => todo!("Figure out what the default AnyRef value is"),
        ValType::FuncRef => todo!("Figure out what the default FuncRef value is"),
    };
    let delta_result = table.grow(delta, table_default_value);
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
pub extern "C" fn wasmer_table_length(table: *mut wasmer_table_t) -> u32 {
    let table = unsafe { &*(table as *mut Table) };
    table.size()
}

/// Frees memory for the given Table
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub extern "C" fn wasmer_table_destroy(table: *mut wasmer_table_t) {
    if !table.is_null() {
        unsafe { Box::from_raw(table as *mut Table) };
    }
}
