use crate::error::update_last_error;

use super::super::store::{StoreRef, wasm_store_t};
use super::super::types::{wasm_ref_t, wasm_table_size_t, wasm_tabletype_t};
use super::wasm_extern_t;
use wasmer_api::{Extern, Table, Type, Value};

#[allow(non_camel_case_types)]
#[repr(C)]
#[derive(Clone)]
pub struct wasm_table_t {
    pub(crate) extern_: wasm_extern_t,
}

impl wasm_table_t {
    pub(crate) fn try_from(e: &wasm_extern_t) -> Option<&wasm_table_t> {
        match &e.inner {
            Extern::Table(_) => Some(unsafe { &*(e as *const _ as *const _) }),
            _ => None,
        }
    }
}

/// Builds the [`Value`] to store into a table slot from a caller-provided
/// reference and the table's element type. A null `init` becomes the null
/// reference of the appropriate kind.
fn init_value(init: *const wasm_ref_t, element_ty: Type) -> Value {
    if init.is_null() {
        match element_ty {
            Type::FuncRef => Value::FuncRef(None),
            _ => Value::ExternRef(None),
        }
    } else {
        // The boxed `wasm_ref_t` carries the authoritative reference value.
        unsafe { &*init }.inner.clone()
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_table_new(
    store: Option<&mut wasm_store_t>,
    table_type: Option<&wasm_tabletype_t>,
    init: *const wasm_ref_t,
) -> Option<Box<wasm_table_t>> {
    let store = store?;
    let table_type = table_type?;
    let table_type = table_type.inner()._table_type;
    let init_val = init_value(init, table_type.ty);

    let table = {
        let mut store_mut = unsafe { store.inner.store_mut() };
        c_try!(Table::new(&mut store_mut, table_type, init_val))
    };

    Some(Box::new(wasm_table_t {
        extern_: wasm_extern_t::new(store.inner.clone(), table.into()),
    }))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_table_delete(_table: Option<Box<wasm_table_t>>) {}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_table_copy(
    table: Option<&wasm_table_t>,
) -> Option<Box<wasm_table_t>> {
    table.cloned().map(Box::new)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_table_size(table: Option<&wasm_table_t>) -> usize {
    let Some(table) = table else {
        update_last_error("table pointer is null");
        return 0;
    };
    let store_ref = unsafe { table.extern_.store.store() };
    table.extern_.table().size(&store_ref) as _
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_table_type(
    table: Option<&wasm_table_t>,
) -> Option<Box<wasm_tabletype_t>> {
    let Some(table) = table else {
        update_last_error("table pointer is null");
        return None;
    };
    let store_ref = unsafe { table.extern_.store.store() };
    Some(Box::new(wasm_tabletype_t::new(
        table.extern_.table().ty(&store_ref),
    )))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_table_same(
    wasm_table1: &wasm_table_t,
    wasm_table2: &wasm_table_t,
) -> bool {
    wasm_table1.extern_.table() == wasm_table2.extern_.table()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_table_get(
    table: Option<&wasm_table_t>,
    index: wasm_table_size_t,
) -> Option<Box<wasm_ref_t>> {
    let Some(table) = table else {
        update_last_error("table pointer is null");
        return None;
    };
    let table_obj = table.extern_.table();
    let mut store: StoreRef = table.extern_.store.clone();
    // `Table::get` returns `None` only for an out-of-bounds index; an in-bounds
    // null element is `Some(ExternRef(None))`, which boxes to a null
    // `wasm_ref_t*` below without registering an error.
    let value = {
        let mut store_mut = unsafe { store.store_mut() };
        match table_obj.get(&mut store_mut, index) {
            Some(value) => value,
            None => {
                update_last_error("table index out of bounds");
                return None;
            }
        }
    };
    wasm_ref_t::new(table.extern_.store.clone(), value)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_table_set(
    table: Option<&mut wasm_table_t>,
    index: wasm_table_size_t,
    r: *mut wasm_ref_t,
) -> bool {
    let Some(table) = table else {
        update_last_error("table pointer is null");
        return false;
    };
    let table_obj = table.extern_.table();
    let mut store: StoreRef = table.extern_.store.clone();
    let mut store_mut = unsafe { store.store_mut() };
    let element_ty = table_obj.ty(&store_mut).ty;
    let value = init_value(r, element_ty);
    match table_obj.set(&mut store_mut, index, value) {
        Ok(()) => true,
        Err(e) => {
            update_last_error(e);
            false
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_table_grow(
    table: &mut wasm_table_t,
    delta: wasm_table_size_t,
    init: *mut wasm_ref_t,
) -> bool {
    let table_obj = table.extern_.table();
    let mut store: StoreRef = table.extern_.store.clone();
    let mut store_mut = unsafe { store.store_mut() };
    let element_ty = table_obj.ty(&store_mut).ty;
    let init_val = init_value(init, element_ty);
    match table_obj.grow(&mut store_mut, delta, init_val) {
        Ok(_) => true,
        Err(e) => {
            update_last_error(e);
            false
        }
    }
}
