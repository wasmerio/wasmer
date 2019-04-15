//! Wasm values.

use libc::{int32_t, int64_t};
use wasmer_runtime::Value;
use wasmer_runtime_core::types::Type;

#[allow(non_camel_case_types)]
#[repr(u32)]
#[derive(Clone)]
pub enum wasmer_value_tag {
    WASM_I32,
    WASM_I64,
    WASM_F32,
    WASM_F64,
}

#[repr(C)]
#[derive(Clone, Copy)]
#[allow(non_snake_case)]
pub union wasmer_value {
    pub I32: int32_t,
    pub I64: int64_t,
    pub F32: f32,
    pub F64: f64,
}

#[repr(C)]
#[derive(Clone)]
pub struct wasmer_value_t {
    pub tag: wasmer_value_tag,
    pub value: wasmer_value,
}

impl From<wasmer_value_t> for Value {
    fn from(v: wasmer_value_t) -> Self {
        unsafe {
            #[allow(unreachable_patterns, non_snake_case)]
            match v {
                wasmer_value_t {
                    tag: wasmer_value_tag::WASM_I32,
                    value: wasmer_value { I32 },
                } => Value::I32(I32),
                wasmer_value_t {
                    tag: wasmer_value_tag::WASM_I64,
                    value: wasmer_value { I64 },
                } => Value::I64(I64),
                wasmer_value_t {
                    tag: wasmer_value_tag::WASM_F32,
                    value: wasmer_value { F32 },
                } => Value::F32(F32),
                wasmer_value_t {
                    tag: wasmer_value_tag::WASM_F64,
                    value: wasmer_value { F64 },
                } => Value::F64(F64),
                _ => panic!("not implemented"),
            }
        }
    }
}

impl From<Value> for wasmer_value_t {
    fn from(val: Value) -> Self {
        match val {
            Value::I32(x) => wasmer_value_t {
                tag: wasmer_value_tag::WASM_I32,
                value: wasmer_value { I32: x },
            },
            Value::I64(x) => wasmer_value_t {
                tag: wasmer_value_tag::WASM_I64,
                value: wasmer_value { I64: x },
            },
            Value::F32(x) => wasmer_value_t {
                tag: wasmer_value_tag::WASM_F32,
                value: wasmer_value { F32: x },
            },
            Value::F64(x) => wasmer_value_t {
                tag: wasmer_value_tag::WASM_F64,
                value: wasmer_value { F64: x },
            },
        }
    }
}

impl From<Type> for wasmer_value_tag {
    fn from(ty: Type) -> Self {
        #[allow(unreachable_patterns)]
        match ty {
            Type::I32 => wasmer_value_tag::WASM_I32,
            Type::I64 => wasmer_value_tag::WASM_I64,
            Type::F32 => wasmer_value_tag::WASM_F32,
            Type::F64 => wasmer_value_tag::WASM_F64,
            _ => panic!("not implemented"),
        }
    }
}

impl From<wasmer_value_tag> for Type {
    fn from(v: wasmer_value_tag) -> Self {
        #[allow(unreachable_patterns)]
        match v {
            wasmer_value_tag::WASM_I32 => Type::I32,
            wasmer_value_tag::WASM_I64 => Type::I64,
            wasmer_value_tag::WASM_F32 => Type::F32,
            wasmer_value_tag::WASM_F64 => Type::F64,
            _ => panic!("not implemented"),
        }
    }
}

impl From<&wasmer_runtime::wasm::Type> for wasmer_value_tag {
    fn from(ty: &Type) -> Self {
        match *ty {
            Type::I32 => wasmer_value_tag::WASM_I32,
            Type::I64 => wasmer_value_tag::WASM_I64,
            Type::F32 => wasmer_value_tag::WASM_F32,
            Type::F64 => wasmer_value_tag::WASM_F64,
        }
    }
}
