use super::io::__wasi_option_t;
use wasmer_derive::ValueType;
use wasmer_wasi_types_generated::wasi::Timestamp;

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
#[repr(C)]
pub struct __wasi_option_timestamp_t {
    pub tag: __wasi_option_t,
    pub u: Timestamp,
}
