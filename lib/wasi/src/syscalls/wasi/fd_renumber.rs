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
    let (_, mut state, inodes) = env.get_memory_and_wasi_state_and_inodes(&ctx, 0);

    let mut fd_map = state.fs.fd_map.write().unwrap();
    let fd_entry = wasi_try!(fd_map.get_mut(&from).ok_or(Errno::Badf));

    fd_entry.ref_cnt.fetch_add(1, Ordering::Acquire);
    let new_fd_entry = Fd {
        // TODO: verify this is correct
        ref_cnt: fd_entry.ref_cnt.clone(),
        offset: fd_entry.offset.clone(),
        rights: fd_entry.rights_inheriting,
        ..*fd_entry
    };

    if let Some(fd_entry) = fd_map.get(&to).cloned() {
        if fd_entry.ref_cnt.fetch_sub(1, Ordering::AcqRel) == 1 {
            wasi_try!(state.fs.close_fd_ext(inodes.deref(), &mut fd_map, to));
        }
    }
    fd_map.insert(to, new_fd_entry);

    Errno::Success
}
