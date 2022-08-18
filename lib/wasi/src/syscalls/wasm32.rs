use crate::syscalls::types::*;
use chrono::prelude::*;
use std::mem;
use wasmer::WasmRef;

pub fn platform_clock_res_get(
    clock_id: wasi_snapshot0::Clockid,
    resolution: WasmRef<__wasi_timestamp_t>,
) -> Result<i64, wasi_snapshot0::Errno> {
    let t_out = match clock_id {
        wasi_snapshot0::Clockid::Monotonic => 10_000_000,
        wasi_snapshot0::Clockid::Realtime => 1,
        wasi_snapshot0::Clockid::ProcessCputimeId => 1,
        wasi_snapshot0::Clockid::ThreadCputimeId => 1,
        _ => return Err(wasi_snapshot0::Errno::Inval),
    };
    Ok(t_out)
}

pub fn platform_clock_time_get(
    clock_id: wasi_snapshot0::Clockid,
    precision: __wasi_timestamp_t,
) -> Result<i64, wasi_snapshot0::Errno> {
    let new_time: DateTime<Local> = Local::now();
    Ok(new_time.timestamp_nanos() as i64)
}
