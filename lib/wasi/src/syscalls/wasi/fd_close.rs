use super::*;
use crate::syscalls::*;

/// ### `fd_close()`
/// Close an open file descriptor
/// Inputs:
/// - `Fd fd`
///     A file descriptor mapping to an open file to close
/// Errors:
/// - `Errno::Isdir`
///     If `fd` is a directory
/// - `Errno::Badf`
///     If `fd` is invalid or not open
pub fn fd_close(ctx: FunctionEnvMut<'_, WasiEnv>, fd: WasiFd) -> Errno {
    debug!(
        "wasi[{}:{}]::fd_close: fd={}",
        ctx.data().pid(),
        ctx.data().tid(),
        fd
    );
    let env = ctx.data();
    let (_, mut state, inodes) = env.get_memory_and_wasi_state_and_inodes(&ctx, 0);

    let fd_entry = wasi_try!(state.fs.get_fd(fd));
    wasi_try!(state.fs.close_fd(inodes.deref(), fd));

    Errno::Success
}
