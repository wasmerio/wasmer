/// Utilities to convert between native `wasmi` and `wasmer` values.
use ::wasmi as wasmi_native;

use crate::{BackendFunction, Function, Value};
use wasmer_types::Type;

pub trait IntoCApiValue {
    fn into_cv(self) -> wasmi_native::Val;
}

impl IntoCApiValue for Value {
    fn into_cv(self) -> wasmi_native::Val {
        match self {
            Self::I32(val) => wasmi_native::Val::I32(val),
            Self::I64(val) => wasmi_native::Val::I64(val),
            Self::F32(val) => wasmi_native::Val::F32(val.into()),
            Self::F64(val) => wasmi_native::Val::F64(val.into()),
            Self::FuncRef(Some(val)) => {
                wasmi_native::Val::FuncRef(wasmi_native::Ref::Val(val.as_wasmi().handle))
            }
            Self::FuncRef(None) => wasmi_native::Val::FuncRef(wasmi_native::Ref::Null),
            Self::ExternRef(_) => {
                panic!("Creating host values from guest ExternRefs is not currently supported in wasmi.")
            }
            Self::ExceptionRef(_) => {
                panic!("Creating host values from guest ExceptionRefs is not currently supported in wasmi.")
            }
            Self::V128(_) => {
                panic!("Creating host values from guest V128s is not currently supported in wasmi.")
            }
        }
    }
}

pub trait IntoWasmerValue {
    fn into_wv(self) -> Value;
}

impl IntoWasmerValue for wasmi_native::Val {
    fn into_wv(self) -> Value {
        match self {
            wasmi_native::Val::I32(v) => Value::I32(v),
            wasmi_native::Val::I64(v) => Value::I64(v),
            wasmi_native::Val::F32(v) => Value::F32(v.into()),
            wasmi_native::Val::F64(v) => Value::F64(v.into()),
            wasmi_native::Val::FuncRef(wasmi_native::Ref::Val(func)) => {
                Value::FuncRef(Some(Function(BackendFunction::Wasmi(
                    crate::backend::wasmi::function::Function { handle: func },
                ))))
            }
            wasmi_native::Val::FuncRef(wasmi_native::Ref::Null) => Value::FuncRef(None),
            wasmi_native::Val::ExternRef(_) => {
                panic!("ExternRefs are not currently supported through native wasmi")
            }
            wasmi_native::Val::V128(_) => {
                panic!("wasmi native backend does not currently support V128 values here")
            }
        }
    }
}

pub trait IntoWasmerType {
    fn into_wt(self) -> Type;
}

impl IntoWasmerType for wasmi_native::ValType {
    fn into_wt(self) -> Type {
        match self {
            wasmi_native::ValType::I32 => Type::I32,
            wasmi_native::ValType::I64 => Type::I64,
            wasmi_native::ValType::F32 => Type::F32,
            wasmi_native::ValType::F64 => Type::F64,
            wasmi_native::ValType::ExternRef => Type::ExternRef,
            wasmi_native::ValType::FuncRef => Type::FuncRef,
            wasmi_native::ValType::V128 => unreachable!("wasmi kind has no matching wasmer_types::Type"),
        }
    }
}

pub trait IntoCApiType {
    fn into_ct(self) -> wasmi_native::ValType;
}

impl IntoCApiType for Type {
    fn into_ct(self) -> wasmi_native::ValType {
        match self {
            Self::I32 => wasmi_native::ValType::I32,
            Self::I64 => wasmi_native::ValType::I64,
            Self::F32 => wasmi_native::ValType::F32,
            Self::F64 => wasmi_native::ValType::F64,
            Self::FuncRef => wasmi_native::ValType::FuncRef,
            Self::ExternRef => wasmi_native::ValType::ExternRef,
            Self::V128 => panic!("wasmi does not support V128!"),
            Self::ExceptionRef => panic!("wasmi does not support exnrefs!"),
        }
    }
}
