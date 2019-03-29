//! Wasm values.

use libc::{int32_t, int64_t};

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
