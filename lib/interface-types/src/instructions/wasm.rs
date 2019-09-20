use std::convert::TryFrom;

#[derive(Debug, PartialEq)]
pub enum Type {
    I32,
    I64,
    F32,
    F64,
    V128,
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum Value {
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
    V128(u128),
}

impl From<&Value> for Type {
    fn from(value: &Value) -> Self {
        match value {
            Value::I32(_) => Type::I32,
            Value::I64(_) => Type::I64,
            Value::F32(_) => Type::F32,
            Value::F64(_) => Type::F64,
            Value::V128(_) => Type::V128,
        }
    }
}

impl Default for Value {
    fn default() -> Self {
        Self::I32(0)
    }
}

macro_rules! from_x_for_value {
    ($native_type:ty, $value_variant:ident) => {
        impl From<$native_type> for Value {
            fn from(n: $native_type) -> Self {
                Self::$value_variant(n)
            }
        }

        impl TryFrom<&Value> for $native_type {
            type Error = &'static str;

            fn try_from(w: &Value) -> Result<Self, Self::Error> {
                match *w {
                    Value::$value_variant(n) => Ok(n),
                    _ => Err("Invalid cast."),
                }
            }
        }
    };
}

from_x_for_value!(i32, I32);
from_x_for_value!(i64, I64);
from_x_for_value!(f32, F32);
from_x_for_value!(f64, F64);
from_x_for_value!(u128, V128);

pub trait Export {
    fn inputs_cardinality(&self) -> usize;
    fn outputs_cardinality(&self) -> usize;
    fn inputs(&self) -> &[Type];
    fn outputs(&self) -> &[Type];
    fn call(&self, arguments: &[Value]) -> Result<Vec<Value>, ()>;
}

pub trait Instance<E>
where
    E: Export,
{
    fn export(&self, export_name: &str) -> Option<&E>;
}

impl Export for () {
    fn inputs_cardinality(&self) -> usize {
        0
    }

    fn outputs_cardinality(&self) -> usize {
        0
    }

    fn inputs(&self) -> &[Type] {
        &[]
    }

    fn outputs(&self) -> &[Type] {
        &[]
    }

    fn call(&self, _arguments: &[Value]) -> Result<Vec<Value>, ()> {
        Err(())
    }
}

impl<E> Instance<E> for ()
where
    E: Export,
{
    fn export(&self, _export_name: &str) -> Option<&E> {
        None
    }
}
