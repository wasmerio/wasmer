use super::{
    wasm_externtype_t, wasm_limits_t, wasm_valtype_delete, wasm_valtype_t, WasmExternType,
};
use wasmer::{ExternType, TableType};

#[allow(non_camel_case_types)]
pub type wasm_table_size_t = u32;

const LIMITS_MAX_SENTINEL: u32 = u32::max_value();

#[derive(Debug, Clone)]
pub(crate) struct WasmTableType {
    pub(crate) table_type: TableType,
}

impl WasmTableType {
    pub(crate) fn new(table_type: TableType) -> Self {
        Self { table_type }
    }
}

#[allow(non_camel_case_types)]
#[derive(Debug)]
#[repr(transparent)]
pub struct wasm_tabletype_t {
    pub(crate) extern_type: wasm_externtype_t,
}

impl wasm_tabletype_t {
    pub(crate) fn new(table_type: TableType) -> Self {
        Self {
            extern_type: wasm_externtype_t::new(ExternType::Table(table_type)),
        }
    }

    pub(crate) fn inner(&self) -> &WasmTableType {
        match &self.extern_type.inner {
            WasmExternType::Table(wasm_table_type) => &wasm_table_type,
            _ => unreachable!("Data corruption: `wasm_tabletype_t` does not contain a table type"),
        }
    }
}

wasm_declare_vec!(tabletype);

#[no_mangle]
pub unsafe extern "C" fn wasm_tabletype_new(
    // own
    valtype: Option<Box<wasm_valtype_t>>,
    limits: &wasm_limits_t,
) -> Option<Box<wasm_tabletype_t>> {
    let valtype = valtype?;
    let max_elements = if limits.max == LIMITS_MAX_SENTINEL {
        None
    } else {
        Some(limits.max as _)
    };
    let table_type = Box::new(wasm_tabletype_t::new(TableType::new(
        (*valtype).into(),
        limits.min as _,
        max_elements,
    )));

    wasm_valtype_delete(Some(valtype));

    Some(table_type)
}

// TODO: fix memory leak
// this function leaks memory because the returned limits pointer is not owned
#[no_mangle]
pub unsafe extern "C" fn wasm_tabletype_limits(
    table_type: &wasm_tabletype_t,
) -> *const wasm_limits_t {
    let table_type = table_type.inner().table_type;

    Box::into_raw(Box::new(wasm_limits_t {
        min: table_type.minimum as _,
        max: table_type.maximum.unwrap_or(LIMITS_MAX_SENTINEL),
    }))
}

// TODO: fix memory leak
// this function leaks memory because the returned limits pointer is not owned
#[no_mangle]
pub unsafe extern "C" fn wasm_tabletype_element(
    table_type: &wasm_tabletype_t,
) -> *const wasm_valtype_t {
    let table_type = table_type.inner().table_type;

    Box::into_raw(Box::new(table_type.ty.into()))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_tabletype_delete(_table_type: Option<Box<wasm_tabletype_t>>) {}
