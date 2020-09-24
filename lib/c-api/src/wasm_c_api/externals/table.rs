use super::super::store::wasm_store_t;
use super::super::types::{wasm_ref_t, wasm_table_size_t, wasm_tabletype_t};
use std::ptr::NonNull;
use wasmer::{Store, Table};

#[repr(C)]
pub struct wasm_table_t {
    /// cbindgen:ignore
    // maybe needs to hold onto instance
    pub(crate) inner: Table,
}

#[no_mangle]
pub unsafe extern "C" fn wasm_table_new(
    store_ptr: Option<NonNull<wasm_store_t>>,
    tt: &wasm_tabletype_t,
    init: *const wasm_ref_t,
) -> Option<Box<wasm_table_t>> {
    let tt = tt.as_tabletype().clone();
    let store_ptr: NonNull<Store> = store_ptr?.cast::<Store>();
    let store = store_ptr.as_ref();

    let init_val = todo!("get val from init somehow");

    let table = c_try!(Table::new(store, tt, init_val));
    Some(Box::new(wasm_table_t { inner: table }))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_table_delete(_table: Option<Box<wasm_table_t>>) {}

#[no_mangle]
pub unsafe extern "C" fn wasm_table_copy(wasm_table: &wasm_table_t) -> Box<wasm_table_t> {
    // do shallow copy
    Box::new(wasm_table_t {
        inner: wasm_table.inner.clone(),
    })
}

#[no_mangle]
pub unsafe extern "C" fn wasm_table_same(
    wasm_table1: &wasm_table_t,
    wasm_table2: &wasm_table_t,
) -> bool {
    wasm_table1.inner.same(&wasm_table2.inner)
}

#[no_mangle]
pub unsafe extern "C" fn wasm_table_size(wasm_table: &wasm_table_t) -> usize {
    wasm_table.inner.size() as _
}

#[no_mangle]
pub unsafe extern "C" fn wasm_table_grow(
    _wasm_table: &mut wasm_table_t,
    _delta: wasm_table_size_t,
    _init: *mut wasm_ref_t,
) -> bool {
    // TODO: maybe need to look at result to return `true`; also maybe report error here
    //wasm_table.inner.grow(delta, init).is_ok()
    todo!("Blocked on transforming ExternRef into a val type")
}
