use crate::syscalls::types::*;
use std::mem;
use wasmer::WasmCell;

pub fn platform_clock_res_get(
    clock_id: __wasi_clockid_t,
    resolution: WasmCell<__wasi_timestamp_t>,
) -> __wasi_errno_t {
    let t_out = 1 * 1_000_000_000;
    resolution.set(t_out as __wasi_timestamp_t);

    // TODO: map output of clock_getres to __wasi_errno_t
    __WASI_ESUCCESS
}

pub fn platform_clock_time_get(
    clock_id: __wasi_clockid_t,
    precision: __wasi_timestamp_t,
    time: WasmCell<__wasi_timestamp_t>,
) -> __wasi_errno_t {
    let t_out = 1 * 1_000_000_000;
    time.set(t_out as __wasi_timestamp_t);

    __WASI_ESUCCESS
}
