use crate::executor::IoFuture;
use crate::sys;
use crate::types::*;

pub async fn write(fd: __wasi_fd_t, data: &[u8]) -> Result<usize, __wasi_errno_t> {
    let iov = __wasi_ciovec_t {
        buf: data.as_ptr() as *mut u8,
        buf_len: data.len() as u32,
    };
    let mut out_len: u32 = 0;
    let err = IoFuture::new(move |uctx| unsafe {
        let mut ct = CancellationToken(0);
        let e = sys::write(fd, &iov, 1, 0, &mut out_len, uctx, &mut ct);
        if e != 0 {
            Err(e)
        } else {
            Ok(ct)
        }
    })
    .await;
    if err != 0 {
        Err(err)
    } else {
        Ok(out_len as _)
    }
}
pub async fn read(fd: __wasi_fd_t, data: &mut [u8]) -> Result<usize, __wasi_errno_t> {
    let iov = __wasi_ciovec_t {
        buf: data.as_mut_ptr(),
        buf_len: data.len() as u32,
    };
    let mut out_len: u32 = 0;
    let err = IoFuture::new(move |uctx| unsafe {
        let mut ct = CancellationToken(0);
        let e = sys::read(
            fd,
            &iov,
            1,
            0,
            &mut out_len,
            std::ptr::null_mut(),
            uctx,
            &mut ct,
        );
        if e != 0 {
            Err(e)
        } else {
            Ok(ct)
        }
    })
    .await;
    if err != 0 {
        Err(err)
    } else {
        Ok(out_len as _)
    }
}
