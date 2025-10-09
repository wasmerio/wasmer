/// Utilities to convert between `wasmi` and `wasmer` values
use crate::{
    BackendFunction, Function, Value,
    wasmi::{
        bindings::{
            self, wasm_func_as_ref, wasm_val_t, wasm_val_t__bindgen_ty_1, wasm_valkind_t,
            wasm_valtype_kind, wasm_valtype_t,
        },
        function,
    },
};
use wasmer_types::Type;

pub trait IntoCApiValue {
    /// Consume [`self`] to produce a [`wasm_val_t`].
    fn into_cv(self) -> wasm_val_t;
}

impl IntoCApiValue for Value {
    fn into_cv(self) -> wasm_val_t {
        match self {
            Self::I32(val) => wasm_val_t {
                kind: bindings::wasm_valkind_enum_WASM_I32 as _,
                of: wasm_val_t__bindgen_ty_1 { i32_: val },
            },
            Self::I64(val) => wasm_val_t {
                kind: bindings::wasm_valkind_enum_WASM_I64 as _,
                of: wasm_val_t__bindgen_ty_1 { i64_: val },
            },
            Self::F32(val) => wasm_val_t {
                kind: bindings::wasm_valkind_enum_WASM_F32 as _,
                of: wasm_val_t__bindgen_ty_1 { f32_: val },
            },
            Self::F64(val) => wasm_val_t {
                kind: bindings::wasm_valkind_enum_WASM_F64 as _,
                of: wasm_val_t__bindgen_ty_1 { f64_: val },
            },
            Self::FuncRef(Some(val)) => wasm_val_t {
                kind: bindings::wasm_valkind_enum_WASM_FUNCREF as _,
                of: wasm_val_t__bindgen_ty_1 {
                    ref_: unsafe { wasm_func_as_ref(val.as_wasmi().handle) },
                },
            },
            Self::FuncRef(None) => wasm_val_t {
                kind: bindings::wasm_valkind_enum_WASM_FUNCREF as _,
                of: wasm_val_t__bindgen_ty_1 {
                    ref_: unsafe { wasm_func_as_ref(std::ptr::null_mut()) },
                },
            },
            Self::ExternRef(_) => panic!(
                "Creating host values from guest ExternRefs is not currently supported in wasmi."
            ),
            Self::ExceptionRef(_) => panic!(
                "Creating host values from guest ExceptionRefs is not currently supported in wasmi."
            ),
            Self::V128(_) => {
                panic!("Creating host values from guest V128s is not currently supported in wasmi.")
            }
        }
    }
}

pub trait IntoWasmerValue {
    /// Consume [`self`] to produce a [`Value`].
    fn into_wv(self) -> Value;
}

impl IntoWasmerValue for wasm_val_t {
    fn into_wv(self) -> Value {
        match self.kind as _ {
            bindings::wasm_valkind_enum_WASM_I32 => Value::I32(unsafe { self.of.i32_ }),
            bindings::wasm_valkind_enum_WASM_I64 => Value::I64(unsafe { self.of.i64_ }),
            bindings::wasm_valkind_enum_WASM_F32 => Value::F32(unsafe { self.of.f32_ }),
            bindings::wasm_valkind_enum_WASM_F64 => Value::F64(unsafe { self.of.f64_ }),
            bindings::wasm_valkind_enum_WASM_FUNCREF => Value::FuncRef(Some(Function(
                BackendFunction::Wasmi(crate::backend::wasmi::function::Function {
                    handle: unsafe { self.of.ref_ as _ },
                }),
            ))),
            bindings::wasm_valkind_enum_WASM_EXTERNREF => {
                panic!("ExternRefs are not currently supported through wasm_c_api")
            }

            _ => {
                panic!("wasmi currently does not support V128 values")
            }
        }
    }
}

pub trait IntoWasmerType {
    /// Consume [`self`] to produce a [`Type`].
    fn into_wt(self) -> Type;
}

impl IntoWasmerType for wasm_valkind_t {
    fn into_wt(self) -> Type {
        match self as _ {
            bindings::wasm_valkind_enum_WASM_I32 => Type::I32,
            bindings::wasm_valkind_enum_WASM_I64 => Type::I64,
            bindings::wasm_valkind_enum_WASM_F32 => Type::F32,
            bindings::wasm_valkind_enum_WASM_F64 => Type::F64,
            bindings::wasm_valkind_enum_WASM_EXTERNREF => Type::ExternRef,
            bindings::wasm_valkind_enum_WASM_FUNCREF => Type::FuncRef,
            _ => unreachable!("wasmi kind has no matching wasmer_types::Type"),
        }
    }
}

pub trait IntoCApiType {
    /// Consume [`self`] to produce a [`wasm_valkind_t`].
    fn into_ct(self) -> wasm_valkind_t;
}

impl IntoCApiType for Type {
    fn into_ct(self) -> wasm_valkind_t {
        match self as _ {
            Self::I32 => bindings::wasm_valkind_enum_WASM_I32 as _,
            Self::I64 => bindings::wasm_valkind_enum_WASM_I64 as _,
            Self::F32 => bindings::wasm_valkind_enum_WASM_F32 as _,
            Self::F64 => bindings::wasm_valkind_enum_WASM_F64 as _,
            Self::FuncRef => bindings::wasm_valkind_enum_WASM_FUNCREF as _,
            Self::ExternRef => bindings::wasm_valkind_enum_WASM_EXTERNREF as _,
            Self::V128 => panic!("wasmi does not support V128!"),
            Self::ExceptionRef => panic!("wasmi does not support exnrefs!"),
        }
    }
}

impl IntoWasmerType for wasm_valtype_t {
    fn into_wt(self) -> Type {
        let type_: wasm_valkind_t = unsafe { wasm_valtype_kind(&self as *const _) };
        type_.into_wt()
    }
}

impl IntoWasmerType for *const wasm_valtype_t {
    fn into_wt(self) -> Type {
        let type_: wasm_valkind_t = unsafe { wasm_valtype_kind(self as _) };
        type_.into_wt()
    }
}

impl IntoWasmerType for *mut wasm_valtype_t {
    fn into_wt(self) -> Type {
        let type_: wasm_valkind_t = unsafe { wasm_valtype_kind(self as _) };
        type_.into_wt()
    }
}
