use crate::syscalls::types::*;
use std::cell::Cell;
use std::mem;

use libc::preadv;

pub fn platform_fd_pread(
    fd: __wasi_fd_t,
    iovs: &[Cell<__wasi_iovec_t>],
    iovs_len: u32,
    offset: __wasi_filesize_t,
    nread: &Cell<u32>,
) -> __wasi_errno_t {
    let (result, iovec) = unsafe {
        let mut iovec = vec![mem::uninitialized(); iovs_len as usize];
        (
            preadv(
                fd as i32,
                iovec.as_mut_ptr(),
                iovs_len as i32,
                offset as i64,
            ),
            iovec,
        )
    };
    nread.set(result as u32);
    /*for (i, arr_cell) in iovs.iter().enumerate() {
        let wasi_iovec = __wasi_iovec_t {
            buf: iovec[i] as _,
            buf_len: iovec[i].iov_len as u32,
        };
        arr_cell.set(wasi_iovec);
    }*/

    __WASI_ESUCCESS
}
