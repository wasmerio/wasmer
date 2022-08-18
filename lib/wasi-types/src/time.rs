use super::io::__wasi_option_t;
use wasmer_derive::ValueType;

pub type __wasi_timestamp_t = u64;

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
#[repr(C)]
pub struct __wasi_option_timestamp_t {
    pub tag: __wasi_option_t,
    pub u: __wasi_timestamp_t,
}
