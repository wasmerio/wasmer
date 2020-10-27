use super::{
    wasm_externtype_t, wasm_mutability_enum, wasm_mutability_t, wasm_valtype_delete, wasm_valtype_t,
};
use std::convert::TryInto;
use wasmer::{ExternType, GlobalType};

#[allow(non_camel_case_types)]
#[derive(Clone, Debug)]
pub struct wasm_globaltype_t {
    pub(crate) extern_: wasm_externtype_t,
}

impl wasm_globaltype_t {
    pub(crate) fn as_globaltype(&self) -> &GlobalType {
        if let ExternType::Global(ref g) = self.extern_.inner {
            g
        } else {
            unreachable!(
                "Data corruption detected: `wasm_globaltype_t` does not contain a `GlobalType`"
            );
        }
    }

    pub(crate) fn new(global_type: GlobalType) -> Self {
        Self {
            extern_: wasm_externtype_t {
                inner: ExternType::Global(global_type),
            },
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
    wasm_globaltype_new_inner(valtype?, mutability)
}

#[no_mangle]
pub unsafe extern "C" fn wasm_globaltype_delete(_globaltype: Option<Box<wasm_globaltype_t>>) {}

unsafe fn wasm_globaltype_new_inner(
    // own
    valtype: Box<wasm_valtype_t>,
    mutability: wasm_mutability_t,
) -> Option<Box<wasm_globaltype_t>> {
    let me: wasm_mutability_enum = mutability.try_into().ok()?;
    let gd = Box::new(wasm_globaltype_t::new(GlobalType::new(
        (*valtype).into(),
        me.into(),
    )));
    wasm_valtype_delete(Some(valtype));

    Some(gd)
}

#[no_mangle]
pub unsafe extern "C" fn wasm_globaltype_mutability(
    globaltype: &wasm_globaltype_t,
) -> wasm_mutability_t {
    let gt = globaltype.as_globaltype();
    wasm_mutability_enum::from(gt.mutability).into()
}

// TODO: fix memory leak
// this function leaks memory because the returned limits pointer is not owned
#[no_mangle]
pub unsafe extern "C" fn wasm_globaltype_content(
    globaltype: &wasm_globaltype_t,
) -> *const wasm_valtype_t {
    let gt = globaltype.as_globaltype();
    Box::into_raw(Box::new(gt.ty.into()))
}
