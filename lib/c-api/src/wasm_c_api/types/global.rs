use super::{
    wasm_externtype_t, wasm_mutability_enum, wasm_mutability_t, wasm_valtype_delete,
    wasm_valtype_t, WasmExternType,
};
use std::convert::TryInto;
use wasmer::{ExternType, GlobalType};

#[derive(Debug, Clone)]
pub(crate) struct WasmGlobalType {
    pub(crate) global_type: GlobalType,
}

impl WasmGlobalType {
    pub(crate) fn new(global_type: GlobalType) -> Self {
        Self { global_type }
    }
}

#[allow(non_camel_case_types)]
#[derive(Debug)]
#[repr(transparent)]
pub struct wasm_globaltype_t {
    pub(crate) extern_type: wasm_externtype_t,
}

impl wasm_globaltype_t {
    pub(crate) fn new(global_type: GlobalType) -> Self {
        Self {
            extern_type: wasm_externtype_t::new(ExternType::Global(global_type)),
        }
    }

    pub(crate) fn inner(&self) -> &WasmGlobalType {
        match &self.extern_type.inner {
            WasmExternType::Global(wasm_global_type) => &wasm_global_type,
            _ => {
                unreachable!("Data corruption: `wasm_globaltype_t` does not contain a global type")
            }
        }
    }
}

wasm_declare_vec!(globaltype);

#[no_mangle]
pub unsafe extern "C" fn wasm_globaltype_new(
    // own
    valtype: Option<Box<wasm_valtype_t>>,
    mutability: wasm_mutability_t,
) -> Option<Box<wasm_globaltype_t>> {
    let valtype = valtype?;
    let mutability: wasm_mutability_enum = mutability.try_into().ok()?;
    let global_type = Box::new(wasm_globaltype_t::new(GlobalType::new(
        (*valtype).into(),
        mutability.into(),
    )));

    wasm_valtype_delete(Some(valtype));

    Some(global_type)
}

#[no_mangle]
pub unsafe extern "C" fn wasm_globaltype_delete(_global_type: Option<Box<wasm_globaltype_t>>) {}

#[no_mangle]
pub unsafe extern "C" fn wasm_globaltype_mutability(
    global_type: &wasm_globaltype_t,
) -> wasm_mutability_t {
    wasm_mutability_enum::from(global_type.inner().global_type.mutability).into()
}

// TODO: fix memory leak
// this function leaks memory because the returned limits pointer is not owned
#[no_mangle]
pub unsafe extern "C" fn wasm_globaltype_content(
    global_type: &wasm_globaltype_t,
) -> *const wasm_valtype_t {
    let global_type = global_type.inner().global_type;

    Box::into_raw(Box::new(global_type.ty.into()))
}
