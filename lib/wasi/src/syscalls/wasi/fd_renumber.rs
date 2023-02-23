use super::*;
use crate::syscalls::*;

/// ### `fd_renumber()`
/// Atomically copy file descriptor
/// Inputs:
/// - `Fd from`
///     File descriptor to copy
/// - `Fd to`
///     Location to copy file descriptor to
pub fn fd_renumber(ctx: FunctionEnvMut<'_, WasiEnv>, from: WasiFd, to: WasiFd) -> Errno {
    debug!(
        "wasi[{}:{}]::fd_renumber(from={}, to={})",
        ctx.data().pid(),
        ctx.data().tid(),
        from,
        to
    );
    if from == to {
        return Errno::Success;
    }
    let env = ctx.data();
    let (_, mut state) = env.get_memory_and_wasi_state(&ctx, 0);

    let mut fd_map = state.fs.fd_map.write().unwrap();
    let fd_entry = wasi_try!(fd_map.get_mut(&from).ok_or(Errno::Badf));

    let new_fd_entry = Fd {
        // TODO: verify this is correct
        offset: fd_entry.offset.clone(),
        rights: fd_entry.rights_inheriting,
        inode: fd_entry.inode.clone(),
        ..*fd_entry
    };
    fd_map.insert(to, new_fd_entry);

    Errno::Success
}
