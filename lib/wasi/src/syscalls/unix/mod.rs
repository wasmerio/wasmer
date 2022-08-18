use crate::syscalls::types::wasi_snapshot0;
use crate::syscalls::types::*;
use libc::{
    clock_getres, clock_gettime, timespec, CLOCK_MONOTONIC, CLOCK_PROCESS_CPUTIME_ID,
    CLOCK_REALTIME, CLOCK_THREAD_CPUTIME_ID,
};
use std::mem;
use wasmer::WasmRef;

pub fn platform_clock_res_get(
    clock_id: wasi_snapshot0::Clockid,
    resolution: WasmRef<__wasi_timestamp_t>,
) -> Result<i64, wasi_snapshot0::Errno> {
    let unix_clock_id = match clock_id {
        wasi_snapshot0::Clockid::Monotonic => CLOCK_MONOTONIC,
        wasi_snapshot0::Clockid::ProcessCputimeId => CLOCK_PROCESS_CPUTIME_ID,
        wasi_snapshot0::Clockid::Realtime => CLOCK_REALTIME,
        wasi_snapshot0::Clockid::ThreadCputimeId => CLOCK_THREAD_CPUTIME_ID,
        _ => return Err(wasi_snapshot0::Errno::Inval),
    };

    let (output, timespec_out) = unsafe {
        let mut timespec_out: timespec = timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        (clock_getres(unix_clock_id, &mut timespec_out), timespec_out)
    };

    let t_out = (timespec_out.tv_sec * 1_000_000_000).wrapping_add(timespec_out.tv_nsec);
    Ok(t_out)
}

pub fn platform_clock_time_get(
    clock_id: wasi_snapshot0::Clockid,
    precision: __wasi_timestamp_t,
) -> Result<i64, wasi_snapshot0::Errno> {
    let unix_clock_id = match clock_id {
        wasi_snapshot0::Clockid::Monotonic => CLOCK_MONOTONIC,
        wasi_snapshot0::Clockid::ProcessCputimeId => CLOCK_PROCESS_CPUTIME_ID,
        wasi_snapshot0::Clockid::Realtime => CLOCK_REALTIME,
        wasi_snapshot0::Clockid::ThreadCputimeId => CLOCK_THREAD_CPUTIME_ID,
        _ => return Err(wasi_snapshot0::Errno::Inval),
    };

    let (output, timespec_out) = unsafe {
        let mut timespec_out: timespec = timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        (
            clock_gettime(unix_clock_id, &mut timespec_out),
            timespec_out,
        )
    };

    let t_out = (timespec_out.tv_sec * 1_000_000_000).wrapping_add(timespec_out.tv_nsec);
    Ok(t_out)
}
