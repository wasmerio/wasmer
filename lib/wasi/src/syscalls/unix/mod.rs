use crate::state::{Kind, WasiFile, WasiFs};
use crate::syscalls::types::*;
use libc::{
    c_int, clock_getres, clock_gettime, nfds_t, poll, timespec, CLOCK_MONOTONIC,
    CLOCK_PROCESS_CPUTIME_ID, CLOCK_REALTIME, CLOCK_THREAD_CPUTIME_ID,
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
        let mut timespec_out: timespec = timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        (clock_getres(unix_clock_id, &mut timespec_out), timespec_out)
    };

    let t_out = (timespec_out.tv_sec * 1_000_000_000).wrapping_add(timespec_out.tv_nsec);
    resolution.set(t_out as __wasi_timestamp_t);

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
    time.set(t_out as __wasi_timestamp_t);

    // TODO: map output of clock_gettime to __wasi_errno_t
    __WASI_ESUCCESS
}

pub fn poll_for_fds() -> Result<(), __wasi_errno_t> {
    //let result = unsafe { poll( , libc::POLLIN, 0 as c_int) };
    unimplemented!()
}

pub fn read_from_fd(
    wasi_fs: &mut WasiFs,
    fd: __wasi_fd_t,
    buffer: &mut [u8],
) -> Result<(), __wasi_errno_t> {
    let fd_entry = wasi_fs.get_fd(fd)?;
    let inode = fd_entry.inode;
    match &mut wasi_fs.inodes[inode].kind {
        Kind::File { handle, .. } => {
            if let Some(h) = handle {

            } else {
                return Err(__WASI_EINVAL);
            }
            unimplemented!()
        }
        Kind::Dir { .. } | Kind::Root { .. } | Kind::Buffer { .. } | Kind::Symlink { .. } => {
            return Err(__WASI_EINVAL)
        }
    }
    let host_fd = unimplemented!();

    let result = unsafe { libc::ioctl(host_fd, libc::FIONREAD, buffer) };
    unimplemented!()
}
