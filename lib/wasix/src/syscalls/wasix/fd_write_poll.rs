use std::task::Waker;

use wasmer_wasix_types::wasi::WakerId;

use super::*;
use crate::{state::conv_waker_id, syscalls::*};

/// ### `fd_write()`
///
/// Polls a write operation on a file descriptor
///
/// If it is not possible to write to the file descriptor at this time then
/// the runtime will register a waker that will be woken when the file is writable again.
///
/// Inputs:
/// - `Fd`
///     File descriptor (opened with writing) to write to
/// - `const __wasi_ciovec_t *iovs`
///     List of vectors to read data from
/// - `u32 iovs_len`
///     Length of data in `iovs`
/// Output:
/// - `u32 *nwritten`
///     Number of bytes written
/// Errors:
///
#[instrument(level = "trace", skip_all, fields(%fd, nwritten = field::Empty), ret, err)]
pub fn fd_write_poll<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    iovs: WasmPtr<__wasi_ciovec_t<M>, M>,
    iovs_len: M::Offset,
    waker: WakerId,
    nwritten: WasmPtr<M::Offset, M>,
) -> Result<Errno, WasiError> {
    let offset = {
        let mut env = ctx.data();
        let state = env.state.clone();
        let inodes = state.inodes.clone();

        let fd_entry = wasi_try_ok!(state.fs.get_fd(fd));
        fd_entry.offset.load(Ordering::Acquire) as usize
    };

    let waker = conv_waker_id(ctx.data().state(), waker);
    fd_write_internal::<M>(
        ctx,
        fd,
        iovs,
        iovs_len,
        offset,
        nwritten,
        true,
        Some(&waker),
    )
}

/// ### `fd_pwrite_poll()`
/// Polls to write to a file without adjusting its offset
///
/// If the write is blocked then a waker will be registered
/// and it will be woken when the file is available for writes
/// again
///
/// Inputs:
/// - `Fd`
///     File descriptor (opened with writing) to write to
/// - `const __wasi_ciovec_t *iovs`
///     List of vectors to read data from
/// - `u32 iovs_len`
///     Length of data in `iovs`
/// - `Filesize offset`
///     The offset to write at
/// Output:
/// - `u32 *nwritten`
///     Number of bytes written
#[instrument(level = "trace", skip_all, fields(%fd, %offset, nwritten = field::Empty), ret, err)]
pub fn fd_pwrite_poll<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    iovs: WasmPtr<__wasi_ciovec_t<M>, M>,
    iovs_len: M::Offset,
    offset: Filesize,
    waker: WakerId,
    nwritten: WasmPtr<M::Offset, M>,
) -> Result<Errno, WasiError> {
    let waker = conv_waker_id(ctx.data().state(), waker);
    fd_write_internal::<M>(
        ctx,
        fd,
        iovs,
        iovs_len,
        offset as usize,
        nwritten,
        false,
        Some(&waker),
    )
}
