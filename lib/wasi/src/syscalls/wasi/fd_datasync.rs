use super::*;
use crate::syscalls::*;

/// ### `fd_datasync()`
/// Synchronize the file data to disk
/// Inputs:
/// - `Fd fd`
///     The file descriptor to sync
pub fn fd_datasync(mut ctx: FunctionEnvMut<'_, WasiEnv>, fd: WasiFd) -> Result<Errno, WasiError> {
    debug!(
        "wasi[{}:{}]::fd_datasync",
        ctx.data().pid(),
        ctx.data().tid()
    );
    let env = ctx.data();
    let state = env.state.clone();
    let fd_entry = wasi_try_ok!(state.fs.get_fd(fd));
    if !fd_entry.rights.contains(Rights::FD_DATASYNC) {
        return Ok(Errno::Access);
    }

    #[allow(clippy::await_holding_lock)]
    Ok(wasi_try_ok!(__asyncify(&mut ctx, None, async move {
        state.fs.flush(fd).await.map(|_| Errno::Success)
    })?))
}
