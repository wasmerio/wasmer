//! `fd_read`, `fd_write`, `fd_close`, through ordinary v2 dispatch.

use crate::abi::{GuestSize, Pointer};

use crate::abi::fs::Rights;

use wasixv2_abi::Fd;
use wasixv2_syscalls::{Control, IoVectorSource, Syscalls, VectorOperation};
use wasmer::RuntimeError;

use crate::shim::{self, Env};

pub(crate) async fn fd_read<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    iovs: Pointer,
    iovs_len: GuestSize,
    nread: Pointer,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        crate::rights::require(frame, &[(wasixv2_abi::Fd(fd), Rights::FD_READ)]).await?;
        let vectors =
            crate::vectors::Vectors::read(frame, VectorOperation::Read, Fd(fd), iovs, iovs_len)
                .await?;
        let caller = frame.caller().with_io_vectors(&vectors);
        let result = frame
            .syscalls
            .readv(&caller, Fd(fd), vectors.array(), vectors.count())
            .await;
        let n = vectors.finish(result)?;
        frame.write_size(nread, n).await
    })
    .await
}

pub(crate) async fn fd_write<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    iovs: Pointer,
    iovs_len: GuestSize,
    nwritten: Pointer,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        crate::rights::require(frame, &[(wasixv2_abi::Fd(fd), Rights::FD_WRITE)]).await?;
        let vectors =
            crate::vectors::Vectors::read(frame, VectorOperation::Write, Fd(fd), iovs, iovs_len)
                .await?;
        let caller = frame.caller().with_io_vectors(&vectors);
        let result = frame
            .syscalls
            .writev(&caller, Fd(fd), vectors.array(), vectors.count())
            .await;
        let n = vectors.finish(result)?;
        frame.write_size(nwritten, n).await
    })
    .await
}

pub(crate) async fn fd_close<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        // v1 never closed its preopened directories (see `fs::is_preopen`).
        if crate::calls::fs::is_preopen(frame, fd).await? {
            return Ok(());
        }
        frame.syscalls.close(&frame.caller(), Fd(fd)).await?;
        crate::calls::epoll::forgotten(frame, Fd(fd)).await
    })
    .await
}
