extern crate wasmer_runtime;
extern crate wasmer_runtime_core;

use libc::{uint32_t, uint8_t};
use wasmer_runtime::Value;
use wasmer_runtime_core::types::Type;

pub mod error;
pub mod export;
pub mod global;
pub mod import;
pub mod instance;
pub mod memory;
pub mod module;
pub mod table;
pub mod value;

use instance::wasmer_instance_t;
use value::{wasmer_value, wasmer_value_t, wasmer_value_tag};

#[allow(non_camel_case_types)]
#[repr(C)]
pub enum wasmer_result_t {
    WASMER_OK = 1,
    WASMER_ERROR = 2,
}

#[repr(C)]
pub struct wasmer_limits_t {
    pub min: uint32_t,
    pub max: wasmer_limit_option_t,
}

#[repr(C)]
pub struct wasmer_limit_option_t {
    pub has_some: bool,
    pub some: uint32_t,
}

#[repr(C)]
pub struct wasmer_byte_array {
    bytes: *const uint8_t,
    bytes_len: uint32_t,
}

impl From<wasmer_value_t> for Value {
    fn from(v: wasmer_value_t) -> Self {
        unsafe {
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
