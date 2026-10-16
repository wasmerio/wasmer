//! `fd_read`, `fd_write`, `fd_close`. v1's `iovec` is Linux's wasm32 `struct iovec`, so the
//! guest's iovec array goes to the kernel untranslated.

use wasixv2_abi::{ConstIovecArray, Fd};
use wasixv2_syscalls::{Control, Syscalls};
use wasmer::RuntimeError;

use crate::shim::{self, Env};

pub(crate) async fn fd_read<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    iovs: i32,
    iovs_len: i32,
    nread: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let iovs = ConstIovecArray::new(iovs as u32);
        let n = frame
            .syscalls
            .readv(&frame.caller(), Fd(fd), iovs, iovs_len as u32)
            .await?;
        frame.write(nread as u32, &n).await
    })
    .await
}

pub(crate) async fn fd_write<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    iovs: i32,
    iovs_len: i32,
    nwritten: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let iovs = ConstIovecArray::new(iovs as u32);
        let n = frame
            .syscalls
            .writev(&frame.caller(), Fd(fd), iovs, iovs_len as u32)
            .await?;
        frame.write(nwritten as u32, &n).await
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
        Ok(())
    })
    .await
}
