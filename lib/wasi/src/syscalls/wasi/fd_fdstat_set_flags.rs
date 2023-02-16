use super::*;
use crate::syscalls::*;

/// ### `fd_fdstat_set_flags()`
/// Set file descriptor flags for a file descriptor
/// Inputs:
/// - `Fd fd`
///     The file descriptor to apply the new flags to
/// - `Fdflags flags`
///     The flags to apply to `fd`
pub fn fd_fdstat_set_flags(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    flags: Fdflags,
) -> Result<Errno, WasiError> {
    debug!(
        "wasi[{}:{}]::fd_fdstat_set_flags (fd={}, flags={:?})",
        ctx.data().pid(),
        ctx.data().tid(),
        fd,
        flags
    );

    {
        let env = ctx.data();
        let (_, mut state, inodes) = env.get_memory_and_wasi_state_and_inodes(&ctx, 0);
        let mut fd_map = state.fs.fd_map.write().unwrap();
        let fd_entry = wasi_try_ok!(fd_map.get_mut(&fd).ok_or(Errno::Badf));
        let inode = fd_entry.inode.clone();

        if !fd_entry.rights.contains(Rights::FD_FDSTAT_SET_FLAGS) {
            debug!(
                "wasi[{}:{}]::fd_fdstat_set_flags (fd={}, flags={:?}) - access denied",
                ctx.data().pid(),
                ctx.data().tid(),
                fd,
                flags
            );
            return Ok(Errno::Access);
        }
    }

    let env = ctx.data();
    let (_, mut state, inodes) = env.get_memory_and_wasi_state_and_inodes(&ctx, 0);
    let mut fd_map = state.fs.fd_map.write().unwrap();
    let fd_entry = wasi_try_ok!(fd_map.get_mut(&fd).ok_or(Errno::Badf));
    fd_entry.flags = flags;
    Ok(Errno::Success)
}
