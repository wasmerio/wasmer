use super::super::value::wasm_valkind_t;
use std::convert::TryInto;
use wasmer_api::Type;

#[allow(non_camel_case_types)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum wasm_valkind_enum {
    WASM_I32 = 0,
    WASM_I64 = 1,
    WASM_F32 = 2,
    WASM_F64 = 3,
    WASM_EXTERNREF = 128,
    WASM_FUNCREF = 129,
    WASM_EXNREF = 130,
}

impl From<Type> for wasm_valkind_enum {
    fn from(other: Type) -> Self {
        match other {
            Type::I32 => Self::WASM_I32,
            Type::I64 => Self::WASM_I64,
            Type::F32 => Self::WASM_F32,
            Type::F64 => Self::WASM_F64,
            Type::V128 => todo!("no v128 type in Wasm C API yet!"),
            Type::ExternRef => Self::WASM_EXTERNREF,
            Type::FuncRef => Self::WASM_FUNCREF,
            Type::ExceptionRef => Self::WASM_EXNREF,
        }
    }
}

impl From<wasm_valkind_enum> for Type {
    fn from(other: wasm_valkind_enum) -> Self {
        use wasm_valkind_enum::*;
        match other {
            WASM_I32 => Type::I32,
            WASM_I64 => Type::I64,
            WASM_F32 => Type::F32,
            WASM_F64 => Type::F64,
            WASM_EXTERNREF => Type::ExternRef,
            WASM_FUNCREF => Type::FuncRef,
            WASM_EXNREF => Type::ExternRef,
        }
    }
}

#[allow(non_camel_case_types)]
#[derive(Debug, Clone, Copy)]
pub struct wasm_valtype_t {
    valkind: wasm_valkind_enum,
}

impl Default for wasm_valtype_t {
    fn default() -> Self {
        Self {
            valkind: wasm_valkind_enum::WASM_I32,
        }
    }
}

wasm_declare_boxed_vec!(valtype);

impl From<wasm_valtype_t> for Type {
    fn from(other: wasm_valtype_t) -> Self {
        (&other).into()
    }
}

impl From<&wasm_valtype_t> for Type {
    fn from(other: &wasm_valtype_t) -> Self {
        other.valkind.into()
    }
}

impl From<Type> for wasm_valtype_t {
    fn from(other: Type) -> Self {
        Self {
            valkind: other.into(),
        }
    }
}

#[no_mangle]
pub extern "C" fn wasm_valtype_new(kind: wasm_valkind_t) -> Option<Box<wasm_valtype_t>> {
    let kind_enum = kind.try_into().ok()?;
    let valtype = wasm_valtype_t { valkind: kind_enum };

    Some(Box::new(valtype))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_valtype_delete(_valtype: Option<Box<wasm_valtype_t>>) {}

#[no_mangle]
pub unsafe extern "C" fn wasm_valtype_kind(valtype: Option<&wasm_valtype_t>) -> wasm_valkind_t {
    valtype
        .expect("`wasm_valtype_kind: argument is a null pointer")
        .valkind as wasm_valkind_t
}
