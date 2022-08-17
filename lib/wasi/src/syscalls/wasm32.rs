use crate::syscalls::types::*;
use chrono::prelude::*;
use std::mem;
use wasmer::WasmRef;

pub fn platform_clock_res_get(
    clock_id: __wasi_clockid_t,
    resolution: WasmRef<__wasi_timestamp_t>,
) -> Result<i64, wasi_snapshot0::Errno> {
    let t_out = match clock_id {
        __WASI_CLOCK_MONOTONIC => 10_000_000,
        __WASI_CLOCK_REALTIME => 1,
        __WASI_CLOCK_PROCESS_CPUTIME_ID => 1,
        __WASI_CLOCK_THREAD_CPUTIME_ID => 1,
        _ => return Err(wasi_snapshot0::Errno::Inval),
    };
    Ok(t_out)
}

pub fn platform_clock_time_get(
    clock_id: __wasi_clockid_t,
    precision: __wasi_timestamp_t,
) -> Result<i64, wasi_snapshot0::Errno> {
    let new_time: DateTime<Local> = Local::now();
    Ok(new_time.timestamp_nanos() as i64)
}
