use crate::syscalls::types::*;
use std::mem;
use wasmer::WasmRef;

pub fn platform_clock_res_get(
    clock_id: __wasi_clockid_t,
    resolution: WasmRef<__wasi_timestamp_t>,
) -> __wasi_errno_t {
    let t_out = 1 * 1_000_000_000;
    wasi_try_mem!(resolution.write(t_out as __wasi_timestamp_t));

    // TODO: map output of clock_getres to __wasi_errno_t
    __WASI_ESUCCESS
}

pub fn platform_clock_time_get(
    clock_id: __wasi_clockid_t,
    precision: __wasi_timestamp_t,
    time: WasmRef<__wasi_timestamp_t>,
) -> __wasi_errno_t {
    let t_out = 1 * 1_000_000_000;
    wasi_try_mem!(time.write(t_out as __wasi_timestamp_t));

    __WASI_ESUCCESS
}
