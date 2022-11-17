use super::*;
use crate::syscalls::*;

/// ### `fd_datasync()`
/// Synchronize the file data to disk
/// Inputs:
/// - `Fd fd`
///     The file descriptor to sync
pub fn fd_datasync(ctx: FunctionEnvMut<'_, WasiEnv>, fd: WasiFd) -> Errno {
    debug!(
        "wasi[{}:{}]::fd_datasync",
        ctx.data().pid(),
        ctx.data().tid()
    );
    let env = ctx.data();
    let (_, _, inodes) = env.get_memory_and_wasi_state_and_inodes(&ctx, 0);
    let state = env.state.clone();
    let fd_entry = wasi_try!(state.fs.get_fd(fd));
    if !fd_entry.rights.contains(Rights::FD_DATASYNC) {
        return Errno::Access;
    }

    wasi_try!(__asyncify(&mut ctx, None, async move {
        state.fs.flush(inodes.deref(), fd).await
            .map(|_| Errno::Success)
    }))
}