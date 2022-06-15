use super::io::__wasi_option_t;
use wasmer_derive::ValueType;

pub type __wasi_clockid_t = u32;
pub const __WASI_CLOCK_REALTIME: __wasi_clockid_t = 0;
pub const __WASI_CLOCK_MONOTONIC: __wasi_clockid_t = 1;
pub const __WASI_CLOCK_PROCESS_CPUTIME_ID: __wasi_clockid_t = 2;
pub const __WASI_CLOCK_THREAD_CPUTIME_ID: __wasi_clockid_t = 3;

pub type __wasi_timestamp_t = u64;

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
#[repr(C)]
pub struct __wasi_option_timestamp_t {
    pub tag: __wasi_option_t,
    pub u: __wasi_timestamp_t,
}
