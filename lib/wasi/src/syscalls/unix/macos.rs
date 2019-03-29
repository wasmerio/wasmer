use crate::syscalls::types::*;
use std::cell::Cell;

pub fn platform_fd_pread(
    fd: __wasi_fd_t,
    iovs: &[Cell<__wasi_iovec_t>],
    iovs_len: u32,
    offset: __wasi_filesize_t,
    nread: &Cell<u32>,
) -> __wasi_errno_t {
    unimplemented!()
}
