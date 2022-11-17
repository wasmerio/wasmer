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
pub fn fd_sync(mut ctx: FunctionEnvMut<'_, WasiEnv>, fd: WasiFd) -> Errno {
    debug!("wasi[{}:{}]::fd_sync", ctx.data().pid(), ctx.data().tid());
    debug!("=> fd={}", fd);
    let env = ctx.data();
    let (_, mut state, inodes) = env.get_memory_and_wasi_state_and_inodes(&ctx, 0);
    let fd_entry = wasi_try!(state.fs.get_fd(fd));
    if !fd_entry.rights.contains(Rights::FD_SYNC) {
        return Errno::Access;
    }
    let inode = fd_entry.inode;

    // TODO: implement this for more than files
    {
        let mut guard = inodes.arena[inode].write();
        match guard.deref_mut() {
            Kind::File { handle, .. } => {
                if let Some(handle) = handle {
                    let handle = handle.clone();
                    drop(inode);
                    drop(guard);
                    drop(inodes);

                    wasi_try!(__asyncify(&mut ctx, None, async move {
                        let mut handle = handle.write().unwrap();
                        handle.flush().await.map_err(map_io_err)
                    }))
                } else {
                    return Errno::Inval;
                }
            }
            Kind::Root { .. } | Kind::Dir { .. } => return Errno::Isdir,
            Kind::Buffer { .. }
            | Kind::Symlink { .. }
            | Kind::Socket { .. }
            | Kind::Pipe { .. }
            | Kind::EventNotifications { .. } => return Errno::Inval,
        }
    }

    Errno::Success
}
