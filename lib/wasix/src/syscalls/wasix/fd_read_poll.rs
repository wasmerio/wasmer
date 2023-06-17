use std::{collections::VecDeque, task::Waker};

use virtual_fs::{AsyncReadExt, ReadBuf};
use wasmer_wasix_types::wasi::WakerId;

use super::*;
use crate::{fs::NotificationInner, state::conv_waker_id, syscalls::*};

/// ### `fd_read_poll()`
///
/// Polls to read data from file descriptor
///
/// If there is no data available on the file and it would block then instead
/// it will register a waker that will be woken when the file is readable again
///
/// Inputs:
/// - `Fd fd`
///     File descriptor from which data will be read
/// - `const __wasi_iovec_t *iovs`
///     Vectors where data will be stored
/// - `u32 iovs_len`
///     Length of data in `iovs`
/// Output:
/// - `u32 *nread`
///     Number of bytes read
///
#[instrument(level = "trace", skip_all, fields(%fd, nread = field::Empty), ret, err)]
pub fn fd_read_poll<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    iovs: WasmPtr<__wasi_iovec_t<M>, M>,
    iovs_len: M::Offset,
    waker: WakerId,
    nread: WasmPtr<M::Offset, M>,
) -> Result<Errno, WasiError> {
    let offset = {
        let mut env = ctx.data();
        let state = env.state.clone();
        let inodes = state.inodes.clone();

        let fd_entry = wasi_try_ok!(state.fs.get_fd(fd));
        fd_entry.offset.load(Ordering::Acquire) as usize
    };

    let waker = conv_waker_id(ctx.data().state(), waker);
    let res = fd_read_internal::<M>(
        &mut ctx,
        fd,
        iovs,
        iovs_len,
        offset,
        nread,
        true,
        Some(&waker),
    )?;

    fd_read_internal_handler::<M>(ctx, res, nread)
}

/// ### `fd_pread_poll()`
///
/// Polls to read from the file at the given offset without updating the file cursor.
/// This acts like a stateless version of Seek + Read
///
/// If there is no data available on the file and it would block then instead
/// it will register a waker that will be woken when the file is readable again
///
/// Inputs:
/// - `Fd fd`
///     The file descriptor to read the data with
/// - `const __wasi_iovec_t* iovs'
///     Vectors where the data will be stored
/// - `size_t iovs_len`
///     The number of vectors to store the data into
/// - `Filesize offset`
///     The file cursor to use: the starting position from which data will be read
/// Output:
/// - `size_t nread`
///     The number of bytes read
#[instrument(level = "trace", skip_all, fields(%fd, %offset, ?nread), ret, err)]
pub fn fd_pread_poll<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    iovs: WasmPtr<__wasi_iovec_t<M>, M>,
    iovs_len: M::Offset,
    offset: Filesize,
    waker: WakerId,
    nread: WasmPtr<M::Offset, M>,
) -> Result<Errno, WasiError> {
    let waker = conv_waker_id(ctx.data().state(), waker);
    let res = fd_read_internal::<M>(
        &mut ctx,
        fd,
        iovs,
        iovs_len,
        offset as usize,
        nread,
        false,
        Some(&waker),
    )?;

    fd_read_internal_handler::<M>(ctx, res, nread)
}
