//! Defines WIT values and associated operations.

pub use crate::ast::{InterfaceType, RecordType};
use crate::errors::WasmValueNativeCastError;
pub use crate::interpreter::wasm::serde::*;
use std::convert::TryFrom;

/// A WIT value.
#[derive(Debug, Clone, PartialEq)]
pub enum InterfaceValue {
    /// A 8-bits signed integer.
    S8(i8),

    /// A 16-bits signed integer.
    S16(i16),

    /// A 32-bits signed integer.
    S32(i32),

    /// A 64-bits signed integer.
    S64(i64),

    /// A 8-bits unsigned integer.
    U8(u8),

    /// A 16-bits unsigned integer.
    U16(u16),

    /// A 32-bits unsigned integer.
    U32(u32),

    /// A 64-bits unsigned integer.
    U64(u64),

    /// A 32-bits float.
    F32(f32),

    /// A 64-bits float.
    F64(f64),

    /// A string.
    String(String),

    //Anyref(?),
    /// A 32-bits integer (as defined in WebAssembly core).
    I32(i32),

    /// A 64-bits integer (as defiend in WebAssembly core).
    I64(i64),

    /// A record.
    Record(Vec<InterfaceValue>),
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
            InterfaceValue::Record(values) => Self::Record(RecordType {
                fields: values.iter().map(Into::into).collect(),
            }),
        }
    }
}

impl Default for InterfaceValue {
    fn default() -> Self {
        Self::I32(0)
    }
}

/// Represents a native type supported by WIT.
pub trait NativeType {
    /// The associated interface type that maps to the native type.
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
native!(i32, I32);
native!(i64, I64);
native!(u8, U8);
native!(u16, U16);
native!(u32, U32);
native!(u64, U64);
native!(f32, F32);
native!(f64, F64);
native!(String, String);

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! value_to_type {
        ($test_name:ident, $ty:ident, $value:expr) => {
            #[test]
            #[allow(non_snake_case)]
            fn $test_name() {
                assert_eq!(
                    InterfaceType::from(&InterfaceValue::$ty($value)),
                    InterfaceType::$ty
                );
            }
        };
    }

    value_to_type!(interface_type_from_interface_value__s8, S8, 42);
    value_to_type!(interface_type_from_interface_value__s16, S16, 42);
    value_to_type!(interface_type_from_interface_value__s32, S32, 42);
    value_to_type!(interface_type_from_interface_value__s64, S64, 42);
    value_to_type!(interface_type_from_interface_value__u8, U8, 42);
    value_to_type!(interface_type_from_interface_value__u16, U16, 42);
    value_to_type!(interface_type_from_interface_value__u32, U32, 42);
    value_to_type!(interface_type_from_interface_value__u64, U64, 42);
    value_to_type!(interface_type_from_interface_value__f32, F32, 42.);
    value_to_type!(interface_type_from_interface_value__f64, F64, 42.);
    value_to_type!(
        interface_type_from_interface_value__string,
        String,
        "foo".to_string()
    );
    value_to_type!(interface_type_from_interface_value__i32, I32, 42);
    value_to_type!(interface_type_from_interface_value__i64, I64, 42);

    #[test]
    #[allow(non_snake_case)]
    fn interface_type_from_interface_value__record() {
        assert_eq!(
            InterfaceType::from(&InterfaceValue::Record(vec![
                InterfaceValue::I32(1),
                InterfaceValue::S8(2)
            ])),
            InterfaceType::Record(RecordType {
                fields: vec![InterfaceType::I32, InterfaceType::S8]
            })
        );

        assert_eq!(
            InterfaceType::from(&InterfaceValue::Record(vec![
                InterfaceValue::I32(1),
                InterfaceValue::Record(vec![
                    InterfaceValue::String("a".to_string()),
                    InterfaceValue::F64(42.)
                ]),
                InterfaceValue::S8(2)
            ])),
            InterfaceType::Record(RecordType {
                fields: vec![
                    InterfaceType::I32,
                    InterfaceType::Record(RecordType {
                        fields: vec![InterfaceType::String, InterfaceType::F64]
                    }),
                    InterfaceType::S8
                ]
            })
        );
    }
}
