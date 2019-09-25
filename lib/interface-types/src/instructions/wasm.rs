use std::{cell::Cell, convert::TryFrom};

pub use crate::ast::InterfaceType;

#[derive(Debug, Clone, PartialEq)]
pub enum InterfaceValue {
    Int(isize),
    Float(f64),
    Any(isize),
    String(String),
    // Seq(…),
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
    // AnyRef(…),
}

impl From<&InterfaceValue> for InterfaceType {
    fn from(value: &InterfaceValue) -> Self {
        match value {
            InterfaceValue::Int(_) => Self::Int,
            InterfaceValue::Float(_) => Self::Float,
            InterfaceValue::Any(_) => Self::Any,
            InterfaceValue::String(_) => Self::String,
            InterfaceValue::I32(_) => Self::I32,
            InterfaceValue::I64(_) => Self::I64,
            InterfaceValue::F32(_) => Self::F32,
            InterfaceValue::F64(_) => Self::F64,
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
                match *w {
                    InterfaceValue::$value_variant(n) => Ok(n),
                    _ => Err("Invalid cast."),
                }
            }
        }
    };
}

from_x_for_interface_value!(i32, I32);
from_x_for_interface_value!(i64, I64);
from_x_for_interface_value!(f32, F32);
from_x_for_interface_value!(f64, F64);

pub trait ValueType: Copy
where
    Self: Sized,
{
}

macro_rules! value_type {
    ($native_type:ty) => {
        impl ValueType for $native_type {}
    };

    ($($native_type:ty),*) => {
        $(
            value_type!($native_type);
        )*
    };
}

value_type!(u8, i8, u16, i16, u32, i32, u64, i64, f32, f64);

pub trait TypedIndex: Copy + Clone {
    fn new(index: usize) -> Self;
    fn index(&self) -> usize;
}

macro_rules! typed_index {
    ($type:ident) => {
        #[derive(Copy, Clone)]
        pub struct $type(usize);

        impl TypedIndex for $type {
            fn new(index: usize) -> Self {
                Self(index)
            }

            fn index(&self) -> usize {
                self.0
            }
        }
    };
}

typed_index!(FunctionIndex);
typed_index!(LocalFunctionIndex);
typed_index!(ImportFunctionIndex);

pub trait LocalImportIndex {
    type Local: TypedIndex;
    type Import: TypedIndex;
}

impl LocalImportIndex for FunctionIndex {
    type Local = LocalFunctionIndex;
    type Import = ImportFunctionIndex;
}

pub trait Export {
    fn inputs_cardinality(&self) -> usize;
    fn outputs_cardinality(&self) -> usize;
    fn inputs(&self) -> &[InterfaceType];
    fn outputs(&self) -> &[InterfaceType];
    fn call(&self, arguments: &[InterfaceValue]) -> Result<Vec<InterfaceValue>, ()>;
}

pub trait LocalImport {
    fn inputs_cardinality(&self) -> usize;
    fn outputs_cardinality(&self) -> usize;
    fn inputs(&self) -> &[InterfaceType];
    fn outputs(&self) -> &[InterfaceType];
    fn call(&self, arguments: &[InterfaceValue]) -> Result<Vec<InterfaceValue>, ()>;
}

pub trait Memory {
    fn view<V: ValueType>(&self) -> &[Cell<V>];
}

pub trait Instance<E, LI, M>
where
    E: Export,
    LI: LocalImport,
    M: Memory,
{
    fn export(&self, export_name: &str) -> Option<&E>;
    fn local_or_import<I: TypedIndex + LocalImportIndex>(&self, index: I) -> Option<&LI>;
    fn memory(&self, index: usize) -> Option<&M>;
}

impl Export for () {
    fn inputs_cardinality(&self) -> usize {
        0
    }

    fn outputs_cardinality(&self) -> usize {
        0
    }

    fn inputs(&self) -> &[InterfaceType] {
        &[]
    }

    fn outputs(&self) -> &[InterfaceType] {
        &[]
    }

    fn call(&self, _arguments: &[InterfaceValue]) -> Result<Vec<InterfaceValue>, ()> {
        Err(())
    }
}

impl LocalImport for () {
    fn inputs_cardinality(&self) -> usize {
        0
    }

    fn outputs_cardinality(&self) -> usize {
        0
    }

    fn inputs(&self) -> &[InterfaceType] {
        &[]
    }

    fn outputs(&self) -> &[InterfaceType] {
        &[]
    }

    fn call(&self, _arguments: &[InterfaceValue]) -> Result<Vec<InterfaceValue>, ()> {
        Err(())
    }
}

impl Memory for () {
    fn view<V: ValueType>(&self) -> &[Cell<V>] {
        &[]
    }
}

impl<E, LI, M> Instance<E, LI, M> for ()
where
    E: Export,
    LI: LocalImport,
    M: Memory,
{
    fn export(&self, _export_name: &str) -> Option<&E> {
        None
    }

    fn memory(&self, _: usize) -> Option<&M> {
        None
    }

    fn local_or_import<I: TypedIndex + LocalImportIndex>(&self, _index: I) -> Option<&LI> {
        None
    }
}
