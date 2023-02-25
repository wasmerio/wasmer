use super::*;
use crate::syscalls::*;

/// ### `fd_close()`
/// Close an open file descriptor
/// For sockets this will flush the data before the socket is closed
/// Inputs:
/// - `Fd fd`
///     A file descriptor mapping to an open file to close
/// Errors:
/// - `Errno::Isdir`
///     If `fd` is a directory
/// - `Errno::Badf`
///     If `fd` is invalid or not open
pub fn fd_close(mut ctx: FunctionEnvMut<'_, WasiEnv>, fd: WasiFd) -> Result<Errno, WasiError> {
    debug!(
        "wasi[{}:{}]::fd_close: fd={}",
        ctx.data().pid(),
        ctx.data().tid(),
        fd
    );

    let env = ctx.data();
    let (_, mut state) = env.get_memory_and_wasi_state(&ctx, 0);
    wasi_try_ok!(state.fs.close_fd(fd));

    Ok(Errno::Success)
}
