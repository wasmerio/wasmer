use std::fmt;
use super::super::store::wasm_store_t;
use super::super::types::{wasm_table_size_t, wasm_tabletype_t};
use super::wasm_extern_t;
use super::wasm_func_t;
use wasmer_api::{Extern, Table, Value};

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

impl fmt::Debug for wasm_table_t {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "wasm_table_t({:?}", self.extern_)
    }
}


#[allow(non_camel_case_types)]
#[repr(C)]
#[derive(Clone)]
pub struct wasm_ref_t {
    pub(crate) inner: Value,
}

impl wasm_ref_t {
    pub(crate) fn try_from(e: &Value) -> Option<&wasm_ref_t> {
        Some(unsafe { &*(e as *const _ as *const _) })
    }
}

impl fmt::Debug for wasm_ref_t {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "wasm_ref_t({:?}", self.inner)
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_table_new(
    store: Option<&mut wasm_store_t>,
    table_type: Option<&wasm_tabletype_t>,
    init: Option<&wasm_ref_t>,
) -> Option<Box<wasm_table_t>> {
    let table_type = table_type?;
    let store = store?;
    let mut store_mut = store.inner.store_mut();

    let table_type = &table_type.inner().table_type;
    let extref = if init.is_some() {
        (init.unwrap()).inner.clone()
    } else {
        Value::null()
    };

    let table = Table::new(&mut store_mut, table_type.clone(), extref);
    match table {
        Err(_) => None,
        Ok(table) => {
            let ext = wasm_extern_t::new(store.inner.clone(), Extern::Table(table));
            let table = wasm_table_t::try_from(&ext);
            match table {
                None => None,
                Some(table) => Some(Box::new(table.clone())),
            }
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_table_delete(_table: Option<Box<wasm_table_t>>) {}

#[no_mangle]
pub unsafe extern "C" fn wasm_table_copy(table: &wasm_table_t) -> Box<wasm_table_t> {
    // do shallow copy
    Box::new(table.clone())
}

#[no_mangle]
pub unsafe extern "C" fn wasm_table_size(table: &wasm_table_t) -> usize {
    table.extern_.table().size(&table.extern_.store.store()) as _
}

#[no_mangle]
pub unsafe extern "C" fn wasm_table_same(
    wasm_table1: &wasm_table_t,
    wasm_table2: &wasm_table_t,
) -> bool {
    wasm_table1.extern_.table() == wasm_table2.extern_.table()
}

#[no_mangle]
pub unsafe extern "C" fn wasm_table_grow(
    table: &mut wasm_table_t,
    delta: wasm_table_size_t,
    init: Option<&wasm_ref_t>,
) -> bool {
    table
        .extern_
        .table()
        .grow(
            &mut table.extern_.store.store_mut(),
            delta,
            if init.is_some() {
                init.unwrap().inner.clone()
            } else {
                Value::null_func()
            },
        )
        .is_ok()
}

#[no_mangle]
pub unsafe extern "C" fn wasm_table_get(
    table: &mut wasm_table_t,
    index: wasm_table_size_t,
) -> Option<Box<wasm_ref_t>> {
    let extref = table
        .extern_
        .table()
        .get(&mut table.extern_.store.store_mut(), index);
    extref.and_then(|extref| match extref {
        Value::ExternRef(Some(_)) | Value::FuncRef(Some(_)) => {
            wasm_ref_t::try_from(&extref).and_then(|extref| Some(Box::new(extref.clone())))
        }
        _ => None,
    })
}

#[no_mangle]
pub unsafe extern "C" fn wasm_table_set(
    table: &mut wasm_table_t,
    delta: wasm_table_size_t,
    init: Option<&wasm_ref_t>,
) -> bool {
    println!("table: {:#?}", table);
    println!("delta: {:#?}", delta);
    println!("init: {:#?}", init);
    let val =             if init.is_some() {
        init.unwrap().inner.clone()
    } else {
        Value::null_func()
    };
    table
        .extern_
        .table()
        .set(
            &mut table.extern_.store.store_mut(),
            delta,
            val,
        )
        .is_ok()
}

#[no_mangle]
pub unsafe extern "C" fn wasm_ref_delete(_extref: Option<Box<wasm_ref_t>>) {}

#[no_mangle]
pub unsafe extern "C" fn wasm_func_as_ref(
    func: Option<&wasm_func_t>,
) -> Option<Box<wasm_ref_t>> {

    let func = func?;
    let f = Value::from(func.extern_.function());
    let ret = wasm_ref_t::try_from(&f)?;
    Some(Box::new(ret.clone()))
}
