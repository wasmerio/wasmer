use std::mem;

use libc::{
    clock_getres, clock_gettime, timespec, CLOCK_MONOTONIC, CLOCK_PROCESS_CPUTIME_ID,
    CLOCK_REALTIME, CLOCK_THREAD_CPUTIME_ID,
};
use wasmer::WasmRef;
use wasmer_wasix_types::wasi::{Errno, Snapshot0Clockid, Timestamp};

use crate::syscalls::types::*;

pub fn platform_clock_res_get(
    clock_id: Snapshot0Clockid,
    resolution: WasmRef<Timestamp>,
) -> Result<i64, Errno> {
    let unix_clock_id = match clock_id {
        Snapshot0Clockid::Monotonic => CLOCK_MONOTONIC,
        Snapshot0Clockid::ProcessCputimeId => CLOCK_PROCESS_CPUTIME_ID,
        Snapshot0Clockid::Realtime => CLOCK_REALTIME,
        Snapshot0Clockid::ThreadCputimeId => CLOCK_THREAD_CPUTIME_ID,
        _ => return Err(Errno::Inval),
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
    clock_id: Snapshot0Clockid,
    precision: Timestamp,
) -> Result<i64, Errno> {
    let unix_clock_id = match clock_id {
        Snapshot0Clockid::Monotonic => CLOCK_MONOTONIC,
        Snapshot0Clockid::ProcessCputimeId => CLOCK_PROCESS_CPUTIME_ID,
        Snapshot0Clockid::Realtime => CLOCK_REALTIME,
        Snapshot0Clockid::ThreadCputimeId => CLOCK_THREAD_CPUTIME_ID,
        _ => return Err(Errno::Inval),
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
