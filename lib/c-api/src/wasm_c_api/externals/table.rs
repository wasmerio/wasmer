use super::super::store::wasm_store_t;
use super::super::types::{wasm_ref_t, wasm_table_size_t, wasm_tabletype_t};
use super::CApiExternTag;
use wasmer_api::Table;

#[allow(non_camel_case_types)]
#[repr(C)]
#[derive(Clone)]
pub struct wasm_table_t {
    pub(crate) tag: CApiExternTag,
    pub(crate) inner: Box<Table>,
}

impl wasm_table_t {
    pub(crate) fn new(table: Table) -> Self {
        Self {
            tag: CApiExternTag::Table,
            inner: Box::new(table),
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_table_new(
    _store: Option<&wasm_store_t>,
    _table_type: Option<&wasm_tabletype_t>,
    _init: *const wasm_ref_t,
) -> Option<Box<wasm_table_t>> {
    todo!("get val from init somehow");
}

#[no_mangle]
pub unsafe extern "C" fn wasm_table_delete(_table: Option<Box<wasm_table_t>>) {}

#[no_mangle]
pub unsafe extern "C" fn wasm_table_copy(table: &wasm_table_t) -> Box<wasm_table_t> {
    // do shallow copy
    Box::new(wasm_table_t::new((&*table.inner).clone()))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_table_same(table1: &wasm_table_t, table2: &wasm_table_t) -> bool {
    table1.inner.same(&table2.inner)
}

#[no_mangle]
pub unsafe extern "C" fn wasm_table_size(table: &wasm_table_t) -> usize {
    table.inner.size() as _
}

#[no_mangle]
pub unsafe extern "C" fn wasm_table_grow(
    _table: &mut wasm_table_t,
    _delta: wasm_table_size_t,
    _init: *mut wasm_ref_t,
) -> bool {
    // TODO: maybe need to look at result to return `true`; also maybe report error here
    //wasm_table.inner.grow(delta, init).is_ok()
    todo!("Blocked on transforming ExternRef into a val type")
}
