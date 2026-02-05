use super::*;
use crate::syscalls::*;
use vfs_unix::errno::vfs_error_to_wasi_errno;

/// ### `fd_sync()`
/// Synchronize file and metadata to disk (TODO: expand upon what this means in our system)
/// Inputs:
/// - `Fd fd`
///     The file descriptor to sync
/// Errors:
/// TODO: figure out which errors this should return
/// - `Errno::Perm`
/// - `Errno::Notcapable`
#[instrument(level = "trace", skip_all, fields(%fd), ret)]
pub fn fd_sync(mut ctx: FunctionEnvMut<'_, WasiEnv>, fd: WasiFd) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let env = ctx.data();
    let state = env.state.clone();
    let fd_entry = wasi_try_ok!(state.fs.get_fd(fd));
    if !fd_entry.inner.rights.contains(Rights::FD_SYNC) {
        return Ok(Errno::Access);
    }

    match fd_entry.kind {
        Kind::VfsFile { handle } => {
            let handle = handle.clone();
            #[allow(clippy::await_holding_lock)]
            Ok(wasi_try_ok!(__asyncify(&mut ctx, None, async move {
                handle
                    .fsync()
                    .await
                    .map(|_| Errno::Success)
                    .map_err(|err| vfs_error_to_wasi_errno(&err))
            })?))
        }
        Kind::VfsDir { .. } => Ok(Errno::Isdir),
        Kind::Stdin { .. }
        | Kind::Stdout { .. }
        | Kind::Stderr { .. }
        | Kind::PipeTx { .. }
        | Kind::PipeRx { .. }
        | Kind::DuplexPipe { .. }
        | Kind::Socket { .. }
        | Kind::Epoll { .. }
        | Kind::EventNotifications { .. }
        | Kind::Buffer { .. } => Ok(Errno::Inval),
    }
}
