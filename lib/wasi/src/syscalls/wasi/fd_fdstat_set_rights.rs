use super::*;
use crate::syscalls::*;

/// ### `fd_fdstat_set_rights()`
/// Set the rights of a file descriptor.  This can only be used to remove rights
/// Inputs:
/// - `Fd fd`
///     The file descriptor to apply the new rights to
/// - `Rights fs_rights_base`
///     The rights to apply to `fd`
/// - `Rights fs_rights_inheriting`
///     The inheriting rights to apply to `fd`
pub fn fd_fdstat_set_rights(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    fs_rights_base: Rights,
    fs_rights_inheriting: Rights,
) -> Errno {
    debug!(
        "wasi[{}:{}]::fd_fdstat_set_rights",
        ctx.data().pid(),
        ctx.data().tid()
    );
    let env = ctx.data();
    let (_, mut state) = env.get_memory_and_wasi_state(&ctx, 0);
    let mut fd_map = state.fs.fd_map.write().unwrap();
    let fd_entry = wasi_try!(fd_map.get_mut(&fd).ok_or(Errno::Badf));

    // ensure new rights are a subset of current rights
    if fd_entry.rights | fs_rights_base != fd_entry.rights
        || fd_entry.rights_inheriting | fs_rights_inheriting != fd_entry.rights_inheriting
    {
        return Errno::Notcapable;
    }

    fd_entry.rights = fs_rights_base;
    fd_entry.rights_inheriting = fs_rights_inheriting;

    Errno::Success
}
