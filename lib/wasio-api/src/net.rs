//! Network support.

use crate::executor::IoFuture;
use crate::sys;
use crate::types::*;
use std::mem;

pub fn socket(
    domain: __wasio_socket_domain_t,
    ty: __wasio_socket_type_t,
    protocol: __wasio_socket_protocol_t,
) -> Result<__wasi_fd_t, __wasi_errno_t> {
    let mut fd: __wasi_fd_t = 0;
    let err = unsafe { sys::socket_create(&mut fd, domain, ty, protocol) };
    if err != 0 {
        Err(err)
    } else {
        Ok(fd)
    }
}

pub fn bind4(fd: __wasi_fd_t, sockaddr: &SockaddrIn) -> Result<(), __wasi_errno_t> {
    let err = unsafe {
        sys::socket_bind(
            fd,
            sockaddr as *const _ as *const u8,
            mem::size_of::<SockaddrIn>() as u32,
        )
    };
    if err != 0 {
        Err(err)
    } else {
        Ok(())
    }
}

pub fn bind6(fd: __wasi_fd_t, sockaddr: &SockaddrIn6) -> Result<(), __wasi_errno_t> {
    let err = unsafe {
        sys::socket_bind(
            fd,
            sockaddr as *const _ as *const u8,
            mem::size_of::<SockaddrIn6>() as u32,
        )
    };
    if err != 0 {
        Err(err)
    } else {
        Ok(())
    }
}

pub fn listen(fd: __wasi_fd_t) -> Result<(), __wasi_errno_t> {
    let err = unsafe { sys::socket_listen(fd) };
    if err != 0 {
        Err(err)
    } else {
        Ok(())
    }
}

pub async fn accept(fd: __wasi_fd_t) -> Result<__wasi_fd_t, __wasi_errno_t> {
    let err = IoFuture::new(move |uctx| unsafe {
        let mut ct = CancellationToken(0);
        let e = sys::socket_pre_accept(fd, uctx, &mut ct);
        if e != 0 {
            Err(e)
        } else {
            Ok(ct)
        }
    })
    .await;
    if err != 0 {
        return Err(err);
    }

    let mut conn_fd: __wasi_fd_t = u32::MAX;
    let err = unsafe { sys::socket_accept(&mut conn_fd) };
    if err != 0 {
        Err(err)
    } else {
        Ok(conn_fd)
    }
}

pub fn close(fd: __wasi_fd_t) {
    use std::fs::File;
    use std::os::wasi::prelude::FromRawFd;
    unsafe {
        File::from_raw_fd(fd);
    }
}
