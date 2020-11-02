use super::{wasm_externtype_t, wasm_limits_t, wasm_valtype_delete, wasm_valtype_t};
use wasmer::{ExternType, TableType};

#[allow(non_camel_case_types)]
pub type wasm_table_size_t = u32;

#[allow(non_camel_case_types)]
#[derive(Clone, Debug)]
#[repr(C)]
pub struct wasm_tabletype_t {
    pub(crate) extern_: wasm_externtype_t,
}

wasm_declare_vec!(tabletype);

impl wasm_tabletype_t {
    pub(crate) fn as_tabletype(&self) -> &TableType {
        if let ExternType::Table(ref t) = self.extern_.inner {
            t
        } else {
            unreachable!(
                "Data corruption detected: `wasm_tabletype_t` does not contain a `TableType`"
            );
        }
    }
}

const LIMITS_MAX_SENTINEL: u32 = u32::max_value();

#[no_mangle]
pub unsafe extern "C" fn wasm_tabletype_new(
    // own
    valtype: Box<wasm_valtype_t>,
    limits: &wasm_limits_t,
) -> Box<wasm_tabletype_t> {
    let max_elements = if limits.max == LIMITS_MAX_SENTINEL {
        None
    } else {
        Some(limits.max as _)
    };
    let out = Box::new(wasm_tabletype_t {
        extern_: wasm_externtype_t {
            inner: ExternType::Table(TableType::new(
                (*valtype).into(),
                limits.min as _,
                max_elements,
            )),
        },
    });
    wasm_valtype_delete(Some(valtype));

    out
}

// TODO: fix memory leak
// this function leaks memory because the returned limits pointer is not owned
#[no_mangle]
pub unsafe extern "C" fn wasm_tabletype_limits(
    tabletype: &wasm_tabletype_t,
) -> *const wasm_limits_t {
    let tt = tabletype.as_tabletype();

    Box::into_raw(Box::new(wasm_limits_t {
        min: tt.minimum as _,
        max: tt.maximum.unwrap_or(LIMITS_MAX_SENTINEL),
    }))
}

// TODO: fix memory leak
// this function leaks memory because the returned limits pointer is not owned
#[no_mangle]
pub unsafe extern "C" fn wasm_tabletype_element(
    tabletype: &wasm_tabletype_t,
) -> *const wasm_valtype_t {
    let tt = tabletype.as_tabletype();

    Box::into_raw(Box::new(tt.ty.into()))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_tabletype_delete(_tabletype: Option<Box<wasm_tabletype_t>>) {}
