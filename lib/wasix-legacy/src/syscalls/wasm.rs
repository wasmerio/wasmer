use std::mem;

use chrono::prelude::*;
use wasmer::WasmRef;

use crate::syscalls::types::{
    wasi::{Errno, Snapshot0Clockid, Timestamp},
    *,
};

pub fn platform_clock_res_get(
    clock_id: Snapshot0Clockid,
    resolution: WasmRef<Timestamp>,
) -> Result<i64, Errno> {
    let t_out = match clock_id {
        Snapshot0Clockid::Monotonic => 10_000_000,
        Snapshot0Clockid::Realtime => 1,
        Snapshot0Clockid::ProcessCputimeId => 1,
        Snapshot0Clockid::ThreadCputimeId => 1,
        _ => return Err(Errno::Inval),
    };
    Ok(t_out)
}

pub fn platform_clock_time_get(
    clock_id: Snapshot0Clockid,
    precision: Timestamp,
) -> Result<i64, Errno> {
    Local::now()
        .timestamp_nanos_opt()
        .map(|ts| ts as i64)
        .ok_or(Errno::Overflow)
}
