use crate::syscalls::types::*;
use libc::{
    clock_getres, clock_gettime, timespec, CLOCK_MONOTONIC, CLOCK_PROCESS_CPUTIME_ID,
    CLOCK_REALTIME, CLOCK_THREAD_CPUTIME_ID,
};
use std::cell::Cell;
use std::mem;

pub fn platform_clock_res_get(
    clock_id: __wasi_clockid_t,
    resolution: &Cell<__wasi_timestamp_t>,
) -> __wasi_errno_t {
    let unix_clock_id = match clock_id {
        __WASI_CLOCK_MONOTONIC => CLOCK_MONOTONIC,
        __WASI_CLOCK_PROCESS_CPUTIME_ID => CLOCK_PROCESS_CPUTIME_ID,
        __WASI_CLOCK_REALTIME => CLOCK_REALTIME,
        __WASI_CLOCK_THREAD_CPUTIME_ID => CLOCK_THREAD_CPUTIME_ID,
        _ => return __WASI_EINVAL,
    };

    let (output, timespec_out) = unsafe {
        let mut timespec_out: timespec = mem::uninitialized();
        (clock_getres(unix_clock_id, &mut timespec_out), timespec_out)
    };

    resolution.set(timespec_out.tv_nsec as __wasi_timestamp_t);

    // TODO: map output of clock_getres to __wasi_errno_t
    __WASI_ESUCCESS
}

pub fn platform_clock_time_get(
    clock_id: __wasi_clockid_t,
    precision: __wasi_timestamp_t,
    time: &Cell<__wasi_timestamp_t>,
) -> __wasi_errno_t {
    let unix_clock_id = match clock_id {
        __WASI_CLOCK_MONOTONIC => CLOCK_MONOTONIC,
        __WASI_CLOCK_PROCESS_CPUTIME_ID => CLOCK_PROCESS_CPUTIME_ID,
        __WASI_CLOCK_REALTIME => CLOCK_REALTIME,
        __WASI_CLOCK_THREAD_CPUTIME_ID => CLOCK_THREAD_CPUTIME_ID,
        _ => return __WASI_EINVAL,
    };

    let (output, timespec_out) = unsafe {
        let mut timespec_out: timespec = mem::uninitialized();
        (
            clock_gettime(unix_clock_id, &mut timespec_out),
            timespec_out,
        )
    };

    // TODO: adjust output by precision...

    time.set(timespec_out.tv_nsec as __wasi_timestamp_t);

    // TODO: map output of clock_gettime to __wasi_errno_t
    __WASI_ESUCCESS
}
