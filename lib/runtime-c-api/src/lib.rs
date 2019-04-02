#![deny(warnings)]

extern crate wasmer_runtime;
extern crate wasmer_runtime_core;

use libc::{uint32_t, uint8_t};

pub mod error;
pub mod export;
pub mod global;
pub mod import;
pub mod instance;
pub mod memory;
pub mod module;
pub mod table;
pub mod value;

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
