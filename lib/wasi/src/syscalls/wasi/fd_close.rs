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
    let mut env = ctx.data();
    let fd_entry = wasi_try_ok!(env.state.fs.get_fd(fd));

    let is_non_blocking = fd_entry.flags.contains(Fdflags::NONBLOCK);
    let inode_idx = fd_entry.inode;

    let ret = {
        let (mut memory, _, inodes) = env.get_memory_and_wasi_state_and_inodes(&ctx, 0);
        let inode = &inodes.arena[inode_idx];
        let mut guard = inode.write();
        match guard.deref_mut() {
            Kind::Socket { socket } => {
                let socket = socket.clone();
                drop(guard);
                drop(inodes);
                socket.close()
                    .map(|()| Errno::Success)
                    .unwrap_or_else(|a| a)
            }
            _ => Errno::Success
        }
    };
    
    env = ctx.data();
    let (_, mut state, inodes) = env.get_memory_and_wasi_state_and_inodes(&ctx, 0);
    wasi_try_ok!(state.fs.close_fd(inodes.deref(), fd));

    Ok(ret)
}
