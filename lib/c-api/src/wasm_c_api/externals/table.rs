use super::super::store::wasm_store_t;
use super::super::types::{wasm_ref_t, wasm_table_size_t, wasm_tabletype_t};
use super::wasm_extern_t;
use wasmer_api::Extern;

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

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_table_new(
    _store: Option<&wasm_store_t>,
    _table_type: Option<&wasm_tabletype_t>,
    _init: *const wasm_ref_t,
) -> Option<Box<wasm_table_t>> {
    todo!("get val from init somehow");
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
    let Some(table) = table else { return 0 };
    let store_ref = unsafe { table.extern_.store.store() };
    table.extern_.table().size(&store_ref) as _
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_table_same(
    wasm_table1: &wasm_table_t,
    wasm_table2: &wasm_table_t,
) -> bool {
    wasm_table1.extern_.table() == wasm_table2.extern_.table()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_table_grow(
    _table: &mut wasm_table_t,
    _delta: wasm_table_size_t,
    _init: *mut wasm_ref_t,
) -> bool {
    // TODO: maybe need to look at result to return `true`; also maybe report error here
    //wasm_table.inner.grow(delta, init).is_ok()
    todo!("Blocked on transforming ExternRef into a val type")
}
