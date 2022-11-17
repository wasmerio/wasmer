use super::*;
use crate::syscalls::*;

/// ### `fd_fdstat_set_flags()`
/// Set file descriptor flags for a file descriptor
/// Inputs:
/// - `Fd fd`
///     The file descriptor to apply the new flags to
/// - `Fdflags flags`
///     The flags to apply to `fd`
pub fn fd_fdstat_set_flags(ctx: FunctionEnvMut<'_, WasiEnv>, fd: WasiFd, flags: Fdflags) -> Errno {
    debug!(
        "wasi[{}:{}]::fd_fdstat_set_flags (fd={}, flags={:?})",
        ctx.data().pid(),
        ctx.data().tid(),
        fd,
        flags
    );
    let env = ctx.data();
    let (_, mut state, inodes) = env.get_memory_and_wasi_state_and_inodes(&ctx, 0);
    let mut fd_map = state.fs.fd_map.write().unwrap();
    let fd_entry = wasi_try!(fd_map.get_mut(&fd).ok_or(Errno::Badf));
    let fd_entry = wasi_try!(fd_map.get_mut(&fd).ok_or(Errno::Badf));
    let inode = fd_entry.inode;

    if !fd_entry.rights.contains(Rights::FD_FDSTAT_SET_FLAGS) {
        debug!(
            "wasi[{}:{}]::fd_fdstat_set_flags (fd={}, flags={:?}) - access denied",
            ctx.data().pid(),
            ctx.data().tid(),
            fd,
            flags
        );
        return Errno::Access;
    }

    {
        let mut guard = inodes.arena[inode].write();
        match guard.deref_mut() {
            Kind::Socket { socket } => {
                let nonblocking = flags.contains(Fdflags::NONBLOCK);
                debug!(
                    "wasi[{}:{}]::socket(fd={}) nonblocking={}",
                    ctx.data().pid(),
                    ctx.data().tid(),
                    fd,
                    nonblocking
                );
                socket.set_nonblocking(nonblocking);
            }
            _ => {}
        }
    }

    fd_entry.flags = flags;
    Errno::Success
}
