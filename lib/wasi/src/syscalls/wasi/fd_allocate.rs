use super::*;
use crate::syscalls::*;

/// ### `fd_allocate`
/// Allocate extra space for a file descriptor
/// Inputs:
/// - `Fd fd`
///     The file descriptor to allocate for
/// - `Filesize offset`
///     The offset from the start marking the beginning of the allocation
/// - `Filesize len`
///     The length from the offset marking the end of the allocation
pub fn fd_allocate(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    offset: Filesize,
    len: Filesize,
) -> Errno {
    debug!(
        "wasi[{}:{}]::fd_allocate",
        ctx.data().pid(),
        ctx.data().tid()
    );
    let env = ctx.data();
    let (_, mut state) = env.get_memory_and_wasi_state(&ctx, 0);
    let fd_entry = wasi_try!(state.fs.get_fd(fd));
    let inode = fd_entry.inode;

    if !fd_entry.rights.contains(Rights::FD_ALLOCATE) {
        return Errno::Access;
    }
    let new_size = wasi_try!(offset.checked_add(len).ok_or(Errno::Inval));
    {
        let mut guard = inode.write();
        match guard.deref_mut() {
            Kind::File { handle, .. } => {
                if let Some(handle) = handle {
                    let mut handle = handle.write().unwrap();
                    wasi_try!(handle.set_len(new_size).map_err(fs_error_into_wasi_err));
                } else {
                    return Errno::Badf;
                }
            }
            Kind::Socket { .. } => return Errno::Badf,
            Kind::Pipe { .. } => return Errno::Badf,
            Kind::Buffer { buffer } => {
                buffer.resize(new_size as usize, 0);
            }
            Kind::Symlink { .. } => return Errno::Badf,
            Kind::EventNotifications { .. } => return Errno::Badf,
            Kind::Dir { .. } | Kind::Root { .. } => return Errno::Isdir,
        }
    }
    inode.stat.write().unwrap().st_size = new_size;
    debug!("New file size: {}", new_size);

    Errno::Success
}
