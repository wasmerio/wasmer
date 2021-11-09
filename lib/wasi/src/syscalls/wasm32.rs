use crate::syscalls::types::*;
use std::mem;
use wasmer::WasmCell;

pub fn platform_clock_res_get(
    clock_id: __wasi_clockid_t,
) -> Result<__wasi_timestamp_t, __wasi_errno_t> {
    let t_out = 1 * 1_000_000_000;
    Ok(t_out)
}

pub fn platform_clock_time_get(
    clock_id: __wasi_clockid_t,
    precision: __wasi_timestamp_t,
) -> Result<__wasi_timestamp_t, __wasi_errno_t> {
    let t_out = 1 * 1_000_000_000;
    Ok(t_out)
}
