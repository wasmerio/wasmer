use crate::bindings::{
    wasm_extern_as_ref, wasm_func_as_ref, wasm_val_t, wasm_val_t__bindgen_ty_1,
    wasm_valkind_enum_WASM_EXTERNREF, wasm_valkind_enum_WASM_F32, wasm_valkind_enum_WASM_F64,
    wasm_valkind_enum_WASM_FUNCREF, wasm_valkind_enum_WASM_I32, wasm_valkind_enum_WASM_I64,
    wasm_valkind_enum_WASM_V128, wasm_valkind_t, wasm_valtype_kind, wasm_valtype_t,
};
//use crate::js::externals::Function;
// use crate::store::{Store, StoreObject};
// use crate::js::RuntimeError;
use crate::imports::Imports;
use crate::instance::Instance;
use crate::store::{AsStoreMut, AsStoreRef};
use crate::trap::Trap;
use crate::value::Value;
use crate::{externals, Type};
use crate::{Extern, Function, Global, Memory, Table};
use std::collections::HashMap;
use std::convert::TryInto;
use wasmer_types::ExternType;

/// Convert the given type to a c-api value.
pub trait AsC: Sized {
    /// The inner definition type from this c-api object
    type DefinitionType;
    type OutputType;

    /// Convert the given type to a [`Self::OutputType`].
    fn as_cvalue(&self, store: &impl AsStoreRef) -> Self::OutputType;

    /// Convert the given type to a [`Self::DefinitionType`].
    fn from_cvalue(
        store: &mut impl AsStoreMut,
        type_: &Self::DefinitionType,
        value: &Self::OutputType,
    ) -> Result<Self, Trap>;
}

#[inline]
pub fn param_from_c(value: &wasm_val_t) -> Value {
    match value.kind as u32 {
        wasm_valkind_enum_WASM_I32 => Value::I32(unsafe { value.of.i32_ }),
        wasm_valkind_enum_WASM_I64 => Value::I64(unsafe { value.of.i64_ }),
        wasm_valkind_enum_WASM_F32 => Value::F32(unsafe { value.of.f32_ }),
        wasm_valkind_enum_WASM_F64 => Value::F64(unsafe { value.of.f64_ }),
        _ => unimplemented!("FuncRef and AnyRef aren't implemented as of now"),
    }
}

#[inline]
pub fn result_to_value(param: &Value) -> wasm_val_t {
    match param {
        Value::I32(val) => wasm_val_t {
            kind: wasm_valkind_enum_WASM_I32 as _,
            _paddings: Default::default(),
            of: wasm_val_t__bindgen_ty_1 { i32_: *val },
        },
        Value::I64(val) => wasm_val_t {
            kind: wasm_valkind_enum_WASM_I64 as _,
            _paddings: Default::default(),
            of: wasm_val_t__bindgen_ty_1 { i64_: *val },
        },
        Value::F32(val) => wasm_val_t {
            kind: wasm_valkind_enum_WASM_F32 as _,
            _paddings: Default::default(),
            of: wasm_val_t__bindgen_ty_1 { f32_: *val },
        },
        Value::F64(val) => wasm_val_t {
            kind: wasm_valkind_enum_WASM_F64 as _,
            _paddings: Default::default(),
            of: wasm_val_t__bindgen_ty_1 { f64_: *val },
        },
        Value::FuncRef(val) => wasm_val_t {
            kind: wasm_valkind_enum_WASM_FUNCREF as _,
            _paddings: Default::default(),
            of: wasm_val_t__bindgen_ty_1 {
                ref_: unsafe { wasm_func_as_ref(val.as_ref().unwrap().0.handle) },
            },
        },
        Value::ExternRef(val) => todo!(),
        Value::V128(_) => todo!(),
    }
}

#[inline]
pub fn type_to_c(type_: &Type) -> wasm_valkind_t {
    match type_ {
        Type::I32 => wasm_valkind_enum_WASM_I32 as _,
        Type::I64 => wasm_valkind_enum_WASM_I64 as _,
        Type::F32 => wasm_valkind_enum_WASM_F32 as _,
        Type::F64 => wasm_valkind_enum_WASM_F64 as _,
        Type::FuncRef => wasm_valkind_enum_WASM_FUNCREF as _,
        Type::ExternRef => wasm_valkind_enum_WASM_EXTERNREF as _,
        Type::V128 => wasm_valkind_enum_WASM_V128 as _,
    }
}

#[inline]
pub fn valtype_to_type(type_: *const wasm_valtype_t) -> Type {
    let type_ = unsafe { wasm_valtype_kind(type_) };
    match type_ as u32 {
        wasm_valkind_enum_WASM_I32 => Type::I32,
        wasm_valkind_enum_WASM_I64 => Type::I64,
        wasm_valkind_enum_WASM_F32 => Type::F32,
        wasm_valkind_enum_WASM_F64 => Type::F64,
        wasm_valkind_enum_WASM_V128 => Type::V128,
        wasm_valkind_enum_WASM_EXTERNREF => Type::ExternRef,
        wasm_valkind_enum_WASM_FUNCREF => Type::FuncRef,
        _ => unreachable!(
            "valtype {:?} has no matching valkind and therefore no matching wasmer_types::Type",
            type_
        ),
    }
}
