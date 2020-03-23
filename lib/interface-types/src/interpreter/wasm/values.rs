#![allow(missing_docs)]

pub use crate::ast::InterfaceType;
use crate::errors::WasmValueNativeCastError;
use std::convert::TryFrom;

#[derive(Debug, Clone, PartialEq)]
pub enum InterfaceValue {
    S8(i8),
    S16(i16),
    S32(i32),
    S64(i64),
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    F32(f32),
    F64(f64),
    String(String),
    //Anyref(?),
    I32(i32),
    I64(i64),
}

impl From<&InterfaceValue> for InterfaceType {
    fn from(value: &InterfaceValue) -> Self {
        match value {
            InterfaceValue::S8(_) => Self::S8,
            InterfaceValue::S16(_) => Self::S16,
            InterfaceValue::S32(_) => Self::S32,
            InterfaceValue::S64(_) => Self::S64,
            InterfaceValue::U8(_) => Self::U8,
            InterfaceValue::U16(_) => Self::U16,
            InterfaceValue::U32(_) => Self::U32,
            InterfaceValue::U64(_) => Self::U64,
            InterfaceValue::F32(_) => Self::F32,
            InterfaceValue::F64(_) => Self::F64,
            InterfaceValue::String(_) => Self::String,
            //InterfaceValue::Anyref(_) => Self::Anyref,
            InterfaceValue::I32(_) => Self::I32,
            InterfaceValue::I64(_) => Self::I64,
        }
    }
}

impl Default for InterfaceValue {
    fn default() -> Self {
        Self::I32(0)
    }
}

pub trait NativeType {
    const INTERFACE_TYPE: InterfaceType;
}

macro_rules! native {
    ($native_type:ty, $variant:ident) => {
        impl NativeType for $native_type {
            const INTERFACE_TYPE: InterfaceType = InterfaceType::$variant;
        }

        impl From<$native_type> for InterfaceValue {
            fn from(n: $native_type) -> Self {
                Self::$variant(n)
            }
        }

        impl TryFrom<&InterfaceValue> for $native_type {
            type Error = WasmValueNativeCastError;

            fn try_from(w: &InterfaceValue) -> Result<Self, Self::Error> {
                match w {
                    InterfaceValue::$variant(n) => Ok(n.clone()),
                    _ => Err(WasmValueNativeCastError {
                        from: w.into(),
                        to: <$native_type>::INTERFACE_TYPE,
                    }),
                }
            }
        }
    };
}

native!(i8, S8);
native!(i16, S16);
native!(u8, U8);
native!(u16, U16);
native!(u32, U32);
native!(u64, U64);
native!(f32, F32);
native!(f64, F64);
native!(String, String);
native!(i32, I32);
native!(i64, I64);
