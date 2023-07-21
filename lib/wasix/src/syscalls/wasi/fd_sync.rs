use super::*;
use crate::syscalls::*;

/// ### `fd_sync()`
/// Synchronize file and metadata to disk (TODO: expand upon what this means in our system)
/// Inputs:
/// - `Fd fd`
///     The file descriptor to sync
/// Errors:
/// TODO: figure out which errors this should return
/// - `Errno::Perm`
/// - `Errno::Notcapable`
#[instrument(level = "debug", skip_all, fields(%fd), ret)]
pub fn fd_sync(mut ctx: FunctionEnvMut<'_, WasiEnv>, fd: WasiFd) -> Result<Errno, WasiError> {
    wasi_try_ok!(WasiEnv::process_signals_and_exit(&mut ctx)?);

    let env = ctx.data();
    let (_, mut state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };
    let fd_entry = wasi_try_ok!(state.fs.get_fd(fd));
    if !fd_entry.rights.contains(Rights::FD_SYNC) {
        return Ok(Errno::Access);
    }
    let inode = fd_entry.inode;

    // TODO: implement this for more than files
    {
        let mut guard = inode.write();
        match guard.deref_mut() {
            Kind::File { handle, .. } => {
                if let Some(handle) = handle {
                    let handle = handle.clone();
                    drop(guard);

                    // TODO: remove allow once inodes are refactored (see comments on [`WasiState`])
                    #[allow(clippy::await_holding_lock)]
                    let size = {
                        wasi_try_ok!(__asyncify(&mut ctx, None, async move {
                            // TODO: remove allow once inodes are refactored (see comments on [`WasiState`])
                            #[allow(clippy::await_holding_lock)]
                            let mut handle = handle.write().unwrap();
                            handle.flush().await.map_err(map_io_err)?;
                            Ok(handle.size())
                        })?)
                    };

                    // Update FileStat to reflect the correct current size.
                    // TODO: don't lock twice - currently needed to not keep a lock on all inodes
                    {
                        let env = ctx.data();
                        let (_, mut state, inodes) =
                            unsafe { env.get_memory_and_wasi_state_and_inodes(&ctx, 0) };

                        let fd_entry = wasi_try_ok!(state.fs.get_fd(fd));
                        let inode = fd_entry.inode;
                        let mut guard = inode.stat.write().unwrap();
                        guard.st_size = size;
                    }
                } else {
                    return Ok(Errno::Inval);
                }
            }
            Kind::Root { .. } | Kind::Dir { .. } => return Ok(Errno::Isdir),
            Kind::Buffer { .. }
            | Kind::Symlink { .. }
            | Kind::Socket { .. }
            | Kind::Pipe { .. }
            | Kind::EventNotifications { .. }
            | Kind::Epoll { .. } => return Ok(Errno::Inval),
        }
    }

    Ok(Errno::Success)
}
