#![allow(missing_docs)]

use std::convert::TryFrom;

pub use crate::ast::InterfaceType;

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

macro_rules! from_x_for_interface_value {
    ($native_type:ty, $value_variant:ident) => {
        impl From<$native_type> for InterfaceValue {
            fn from(n: $native_type) -> Self {
                Self::$value_variant(n)
            }
        }

        impl TryFrom<&InterfaceValue> for $native_type {
            type Error = &'static str;

            fn try_from(w: &InterfaceValue) -> Result<Self, Self::Error> {
                match w {
                    InterfaceValue::$value_variant(n) => Ok(n.clone()),
                    _ => Err("Invalid cast."),
                }
            }
        }
    };
}

from_x_for_interface_value!(i8, S8);
from_x_for_interface_value!(i16, S16);
from_x_for_interface_value!(u8, U8);
from_x_for_interface_value!(u16, U16);
from_x_for_interface_value!(u32, U32);
from_x_for_interface_value!(u64, U64);
from_x_for_interface_value!(f32, F32);
from_x_for_interface_value!(f64, F64);
from_x_for_interface_value!(String, String);
from_x_for_interface_value!(i32, I32);
from_x_for_interface_value!(i64, I64);
