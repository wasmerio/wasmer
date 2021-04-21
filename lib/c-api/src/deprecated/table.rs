//! Create, grow, destroy tables of an instance.

use crate::deprecated::{get_global_store, wasmer_limits_t, wasmer_result_t};
use crate::error::update_last_error;
use std::ptr::NonNull;
use wasmer::{ExternRef, Table, TableType, Val, ValType};

#[repr(C)]
#[derive(Clone)]
pub struct wasmer_table_t;

// TODO: this logic should be in wasmer itself
fn get_default_table_value(table_type: ValType) -> Val {
    match table_type {
        ValType::I32 => Val::I32(0),
        ValType::I64 => Val::I64(0),
        ValType::F32 => Val::F32(0.),
        ValType::F64 => Val::F64(0.),
        ValType::V128 => Val::V128(0),
        ValType::ExternRef => Val::ExternRef(ExternRef::null()),
        ValType::FuncRef => Val::FuncRef(None),
    }
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
    table: *mut *mut wasmer_table_t,
    limits: wasmer_limits_t,
) -> wasmer_result_t {
    let max = if limits.max.has_some {
        Some(limits.max.some)
    } else {
        None
    };
    let desc = TableType {
        ty: ValType::FuncRef,
        minimum: limits.min,
        maximum: max,
    };
    let store = get_global_store();
    let result = Table::new(store, desc, get_default_table_value(ValType::FuncRef));
    let new_table = match result {
        Ok(table) => table,
        Err(error) => {
            update_last_error(error);
            return wasmer_result_t::WASMER_ERROR;
        }
    };
    *table = Box::into_raw(Box::new(new_table)) as *mut wasmer_table_t;
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
pub unsafe extern "C" fn wasmer_table_grow(
    table: *mut wasmer_table_t,
    delta: u32,
) -> wasmer_result_t {
    let table = &*(table as *mut Table);
    let table_type = table.ty().ty;
    let table_default_value = get_default_table_value(table_type);
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
pub unsafe extern "C" fn wasmer_table_length(table: *mut wasmer_table_t) -> u32 {
    let table = &*(table as *mut Table);
    table.size()
}

/// Frees memory for the given Table
#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub unsafe extern "C" fn wasmer_table_destroy(table: Option<NonNull<wasmer_table_t>>) {
    if let Some(table_inner) = table {
        Box::from_raw(table_inner.cast::<Table>().as_ptr());
    }
}
