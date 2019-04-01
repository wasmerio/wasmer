use crate::syscalls::types::*;
use std::cell::Cell;

pub fn platform_clock_res_get(
    clock_id: __wasi_clockid_t,
    resolution: &Cell<__wasi_timestamp_t>,
) -> __wasi_errno_t {
    __WASI_EINVAL
}

pub fn platform_clock_time_get(
    clock_id: __wasi_clockid_t,
    precision: __wasi_timestamp_t,
    time: &Cell<__wasi_timestamp_t>,
) -> __wasi_errno_t {
    unimplemented!()
}

pub fn platform_fd_pread(
    fd: __wasi_fd_t,
    iovs: &[Cell<__wasi_iovec_t>],
    iovs_len: u32,
    offset: __wasi_filesize_t,
    nread: &Cell<u32>,
) -> __wasi_errno_t {
    unimplemented!()
}
