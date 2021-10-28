use crate::syscalls::types::*;
use libc::{
    clock_getres, clock_gettime, timespec, CLOCK_MONOTONIC, CLOCK_PROCESS_CPUTIME_ID,
    CLOCK_REALTIME, CLOCK_THREAD_CPUTIME_ID,
};
use std::mem;
use wasmer::WasmRef;

pub fn platform_clock_res_get(
    clock_id: __wasi_clockid_t,
    resolution: WasmRef<__wasi_timestamp_t>,
) -> Result<i64, __wasi_errno_t> {
    let unix_clock_id = match clock_id {
        __WASI_CLOCK_MONOTONIC => CLOCK_MONOTONIC,
        __WASI_CLOCK_PROCESS_CPUTIME_ID => CLOCK_PROCESS_CPUTIME_ID,
        __WASI_CLOCK_REALTIME => CLOCK_REALTIME,
        __WASI_CLOCK_THREAD_CPUTIME_ID => CLOCK_THREAD_CPUTIME_ID,
        _ => return Err(__WASI_EINVAL),
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
    clock_id: __wasi_clockid_t,
    precision: __wasi_timestamp_t,
) -> Result<i64, __wasi_errno_t> {
    let unix_clock_id = match clock_id {
        __WASI_CLOCK_MONOTONIC => CLOCK_MONOTONIC,
        __WASI_CLOCK_PROCESS_CPUTIME_ID => CLOCK_PROCESS_CPUTIME_ID,
        __WASI_CLOCK_REALTIME => CLOCK_REALTIME,
        __WASI_CLOCK_THREAD_CPUTIME_ID => CLOCK_THREAD_CPUTIME_ID,
        _ => return Err(__WASI_EINVAL),
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
