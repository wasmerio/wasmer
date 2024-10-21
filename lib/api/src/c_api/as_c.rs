use crate::bindings::{
    wasm_extern_as_ref, wasm_func_as_ref, wasm_val_t, wasm_val_t__bindgen_ty_1,
    wasm_valkind_enum_WASM_F32, wasm_valkind_enum_WASM_F64, wasm_valkind_enum_WASM_FUNCREF,
    wasm_valkind_enum_WASM_I32, wasm_valkind_enum_WASM_I64, wasm_valkind_t, wasm_valtype_kind,
    wasm_valtype_t,
};

#[cfg(feature = "v8")]
use crate::bindings::wasm_valkind_enum_WASM_ANYREF as wasm_valkind_enum_WASM_EXTERNREF;

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

#[cfg(feature = "v8")]
#[inline]
pub fn param_from_c(value: &wasm_val_t) -> Value {
    match value.kind as _ {
        crate::bindings::wasm_valkind_enum_WASM_I32 => Value::I32(unsafe { value.of.i32_ }),
        crate::bindings::wasm_valkind_enum_WASM_I64 => Value::I64(unsafe { value.of.i64_ }),
        crate::bindings::wasm_valkind_enum_WASM_F32 => Value::F32(unsafe { value.of.f32_ }),
        crate::bindings::wasm_valkind_enum_WASM_F64 => Value::F64(unsafe { value.of.f64_ }),
        crate::bindings::wasm_valkind_enum_WASM_FUNCREF => {
            Value::FuncRef(Some(Function(super::externals::function::Function {
                handle: unsafe { value.of.ref_ as _ },
            })))
        }
        crate::bindings::wasm_valkind_enum_WASM_ANYREF => {
            panic!("ExternRefs are not currently supported through wasm_c_api")
        }
        _ => panic!("v8 curently does not support V128 values"),
    }
}

#[cfg(any(feature = "wamr", feature = "wasmi"))]
#[inline]
pub fn param_from_c(value: &wasm_val_t) -> Value {
    match value.kind as _ {
        crate::bindings::wasm_valkind_enum_WASM_I32 => Value::I32(unsafe { value.of.i32_ }),
        crate::bindings::wasm_valkind_enum_WASM_I64 => Value::I64(unsafe { value.of.i64_ }),
        crate::bindings::wasm_valkind_enum_WASM_F32 => Value::F32(unsafe { value.of.f32_ }),
        crate::bindings::wasm_valkind_enum_WASM_F64 => Value::F64(unsafe { value.of.f64_ }),
        crate::bindings::wasm_valkind_enum_WASM_FUNCREF => {
            Value::FuncRef(Some(Function(super::externals::function::Function {
                handle: unsafe { value.of.ref_ as _ },
            })))
        }
        crate::bindings::wasm_valkind_enum_WASM_EXTERNREF => {
            panic!("ExternRefs are not currently supported through wasm_c_api")
        }

        _ => {
            if cfg!(feature = "wamr") {
                panic!("wamr currently does not support V128 values")
            } else if cfg!(feature = "wamr") {
                panic!("wasmi currently does not support V128 values")
            } else {
                panic!("this backend currently does not support V128 values")
            }
        }
    }
}

#[inline]
pub fn result_to_value(param: &Value) -> wasm_val_t {
    #[cfg(feature = "wamr")]
    {
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
            Value::FuncRef(Some(val)) => wasm_val_t {
                kind: wasm_valkind_enum_WASM_FUNCREF as _,
                _paddings: Default::default(),
                of: wasm_val_t__bindgen_ty_1 {
                    ref_: unsafe { wasm_func_as_ref(val.0.handle) },
                },
            },
            Value::FuncRef(None) => wasm_val_t {
                kind: wasm_valkind_enum_WASM_FUNCREF as _,
                _paddings: Default::default(),
                of: wasm_val_t__bindgen_ty_1 {
                    ref_: unsafe { wasm_func_as_ref(std::ptr::null_mut()) },
                },
            },
            Value::ExternRef(_) => panic!("Creating host values from guest ExternRefs is not currently supported through wasm_c_api.") ,
            Value::V128(_) => panic!("Creating host values from guest V128s is not currently supported through wasm_c_api."),
        }
    }

    #[cfg(any(feature = "wasmi", feature = "v8"))]
    {
        match param {
            Value::I32(val) => wasm_val_t {
                kind: wasm_valkind_enum_WASM_I32 as _,
                of: wasm_val_t__bindgen_ty_1 { i32_: *val },
            },
            Value::I64(val) => wasm_val_t {
                kind: wasm_valkind_enum_WASM_I64 as _,
                of: wasm_val_t__bindgen_ty_1 { i64_: *val },
            },
            Value::F32(val) => wasm_val_t {
                kind: wasm_valkind_enum_WASM_F32 as _,
                of: wasm_val_t__bindgen_ty_1 { f32_: *val },
            },
            Value::F64(val) => wasm_val_t {
                kind: wasm_valkind_enum_WASM_F64 as _,
                of: wasm_val_t__bindgen_ty_1 { f64_: *val },
            },
            Value::FuncRef(Some(val)) => wasm_val_t {
                kind: wasm_valkind_enum_WASM_FUNCREF as _,
                of: wasm_val_t__bindgen_ty_1 {
                    ref_: unsafe { wasm_func_as_ref(val.0.handle) },
                },
            },
            Value::FuncRef(None) => wasm_val_t {
                kind: wasm_valkind_enum_WASM_FUNCREF as _,
                of: wasm_val_t__bindgen_ty_1 {
                    ref_: unsafe { wasm_func_as_ref(std::ptr::null_mut()) },
                },
            },
            Value::ExternRef(_) => panic!("Creating host values from guest ExternRefs is not currently supported through wasm_c_api.") ,
            Value::V128(_) => panic!("Creating host values from guest V128s is not currently supported through wasm_c_api."),
        }
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
        Type::ExternRef => {
            #[cfg(any(feature = "wasmi", feature = "wamr"))]
            {
                crate::bindings::wasm_valkind_enum_WASM_EXTERNREF as _
            }
            #[cfg(feature = "v8")]
            {
                crate::bindings::wasm_valkind_enum_WASM_ANYREF as _
            }
        }
        Type::V128 => {
            #[cfg(feature = "wamr")]
            {
                crate::bindings::wasm_valkind_enum_WASM_V128 as _
            }
            #[cfg(feature = "wasmi")]
            {
                panic!("wasmi does not support V128 kinds as of now");
            }
            #[cfg(feature = "v8")]
            {
                panic!("v8 does not support V128 kinds as of now");
            }
        }
    }
}

#[inline]
pub fn valtype_to_type(type_: *const wasm_valtype_t) -> Type {
    let type_ = unsafe { wasm_valtype_kind(type_) };

    #[cfg(feature = "wamr")]
    match type_ as _ {
        crate::bindings::wasm_valkind_enum_WASM_I32 => Type::I32,
        crate::bindings::wasm_valkind_enum_WASM_I64 => Type::I64,
        crate::bindings::wasm_valkind_enum_WASM_F32 => Type::F32,
        crate::bindings::wasm_valkind_enum_WASM_F64 => Type::F64,
        crate::bindings::wasm_valkind_enum_WASM_V128 => Type::V128,
        crate::bindings::wasm_valkind_enum_WASM_EXTERNREF => Type::ExternRef,
        crate::bindings::wasm_valkind_enum_WASM_FUNCREF => Type::FuncRef,
        _ => unreachable!(
            "valtype {:?} has no matching valkind and therefore no matching wasmer_types::Type",
            type_
        ),
    }
    #[cfg(feature = "wasmi")]
    match type_ as _ {
        crate::bindings::wasm_valkind_enum_WASM_I32 => Type::I32,
        crate::bindings::wasm_valkind_enum_WASM_I64 => Type::I64,
        crate::bindings::wasm_valkind_enum_WASM_F32 => Type::F32,
        crate::bindings::wasm_valkind_enum_WASM_F64 => Type::F64,
        crate::bindings::wasm_valkind_enum_WASM_EXTERNREF => Type::ExternRef,
        crate::bindings::wasm_valkind_enum_WASM_FUNCREF => Type::FuncRef,
        _ => unreachable!(
            "valtype {:?} has no matching valkind and therefore no matching wasmer_types::Type",
            type_
        ),
    }
    #[cfg(feature = "v8")]
    match type_ as _ {
        crate::bindings::wasm_valkind_enum_WASM_I32 => Type::I32,
        crate::bindings::wasm_valkind_enum_WASM_I64 => Type::I64,
        crate::bindings::wasm_valkind_enum_WASM_F32 => Type::F32,
        crate::bindings::wasm_valkind_enum_WASM_F64 => Type::F64,
        crate::bindings::wasm_valkind_enum_WASM_ANYREF => Type::ExternRef,
        crate::bindings::wasm_valkind_enum_WASM_FUNCREF => Type::FuncRef,
        _ => unreachable!(
            "valtype {:?} has no matching valkind and therefore no matching wasmer_types::Type",
            type_
        ),
    }
}
