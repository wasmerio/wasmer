use super::*;
use crate::syscalls::*;

/// ### `fd_filestat_set_size()`
/// Change the size of an open file, zeroing out any new bytes
/// Inputs:
/// - `Fd fd`
///     File descriptor to adjust
/// - `Filesize st_size`
///     New size that `fd` will be set to
pub fn fd_filestat_set_size(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    st_size: Filesize,
) -> Errno {
    debug!(
        "wasi[{}:{}]::fd_filestat_set_size",
        ctx.data().pid(),
        ctx.data().tid()
    );
    let env = ctx.data();
    let (_, mut state) = env.get_memory_and_wasi_state(&ctx, 0);
    let fd_entry = wasi_try!(state.fs.get_fd(fd));
    let inode = fd_entry.inode;

    if !fd_entry.rights.contains(Rights::FD_FILESTAT_SET_SIZE) {
        return Errno::Access;
    }

    {
        let mut guard = inode.write();
        match guard.deref_mut() {
            Kind::File { handle, .. } => {
                if let Some(handle) = handle {
                    let mut handle = handle.write().unwrap();
                    wasi_try!(handle.set_len(st_size).map_err(fs_error_into_wasi_err));
                } else {
                    return Errno::Badf;
                }
            }
            Kind::Buffer { buffer } => {
                buffer.resize(st_size as usize, 0);
            }
            Kind::Socket { .. } => return Errno::Badf,
            Kind::Pipe { .. } => return Errno::Badf,
            Kind::Symlink { .. } => return Errno::Badf,
            Kind::EventNotifications { .. } => return Errno::Badf,
            Kind::Dir { .. } | Kind::Root { .. } => return Errno::Isdir,
        }
    }
    inode.stat.write().unwrap().st_size = st_size;

    Errno::Success
}
