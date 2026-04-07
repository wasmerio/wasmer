/// Utilities to convert between native `wasmi` and `wasmer` values.
use ::wasmi;

use crate::{BackendFunction, Function, Value};
use wasmer_types::Type;

pub trait IntoCApiValue {
    fn into_cv(self) -> wasmi::Val;
}

impl IntoCApiValue for Value {
    fn into_cv(self) -> wasmi::Val {
        match self {
            Self::I32(val) => wasmi::Val::I32(val),
            Self::I64(val) => wasmi::Val::I64(val),
            Self::F32(val) => wasmi::Val::F32(val.into()),
            Self::F64(val) => wasmi::Val::F64(val.into()),
            Self::FuncRef(Some(val)) => {
                wasmi::Val::FuncRef(wasmi::Ref::Val(val.as_wasmi().handle))
            }
            Self::FuncRef(None) => wasmi::Val::FuncRef(wasmi::Ref::Null),
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

impl IntoWasmerValue for wasmi::Val {
    fn into_wv(self) -> Value {
        match self {
            wasmi::Val::I32(v) => Value::I32(v),
            wasmi::Val::I64(v) => Value::I64(v),
            wasmi::Val::F32(v) => Value::F32(v.into()),
            wasmi::Val::F64(v) => Value::F64(v.into()),
            wasmi::Val::FuncRef(wasmi::Ref::Val(func)) => {
                Value::FuncRef(Some(Function(BackendFunction::Wasmi(
                    crate::backend::wasmi::function::Function { handle: func },
                ))))
            }
            wasmi::Val::FuncRef(wasmi::Ref::Null) => Value::FuncRef(None),
            wasmi::Val::ExternRef(_) => {
                panic!("ExternRefs are not currently supported through native wasmi")
            }
            wasmi::Val::V128(_) => {
                panic!("wasmi native backend does not currently support V128 values here")
            }
        }
    }
}

pub trait IntoWasmerType {
    fn into_wt(self) -> Type;
}

impl IntoWasmerType for wasmi::ValType {
    fn into_wt(self) -> Type {
        match self {
            wasmi::ValType::I32 => Type::I32,
            wasmi::ValType::I64 => Type::I64,
            wasmi::ValType::F32 => Type::F32,
            wasmi::ValType::F64 => Type::F64,
            wasmi::ValType::ExternRef => Type::ExternRef,
            wasmi::ValType::FuncRef => Type::FuncRef,
            wasmi::ValType::V128 => unreachable!("wasmi kind has no matching wasmer_types::Type"),
        }
    }
}

pub trait IntoCApiType {
    fn into_ct(self) -> wasmi::ValType;
}

impl IntoCApiType for Type {
    fn into_ct(self) -> wasmi::ValType {
        match self {
            Self::I32 => wasmi::ValType::I32,
            Self::I64 => wasmi::ValType::I64,
            Self::F32 => wasmi::ValType::F32,
            Self::F64 => wasmi::ValType::F64,
            Self::FuncRef => wasmi::ValType::FuncRef,
            Self::ExternRef => wasmi::ValType::ExternRef,
            Self::V128 => panic!("wasmi does not support V128!"),
            Self::ExceptionRef => panic!("wasmi does not support exnrefs!"),
        }
    }
}
