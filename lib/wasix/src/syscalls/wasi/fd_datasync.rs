use super::*;
use crate::fs::FlushPoller;
use crate::syscalls::*;

/// ### `fd_datasync()`
/// Synchronize the file data to disk
/// Inputs:
/// - `Fd fd`
///     The file descriptor to sync
#[instrument(level = "trace", skip_all, fields(%fd), ret)]
pub fn fd_datasync(mut ctx: FunctionEnvMut<'_, WasiEnv>, fd: WasiFd) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let env = ctx.data();
    let (_, state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };
    let fd_entry = wasi_try_ok!(state.fs.get_fd(fd));
    if !fd_entry.inner.rights.contains(Rights::FD_DATASYNC) {
        return Ok(Errno::Access);
    }

    let file = {
        let guard = fd_entry.inode.read();
        match guard.deref() {
            Kind::File {
                handle: Some(file), ..
            } => file.clone(),
            Kind::Dir { .. } => return Ok(Errno::Isdir),
            Kind::Buffer { .. } => return Ok(Errno::Success),
            // Linux fdatasync(2) returns EINVAL for fds bound to pipes, sockets, etc.
            _ => return Ok(Errno::Inval),
        }
    };
    drop(fd_entry);

    Ok(wasi_try_ok!(__asyncify(&mut ctx, None, async move {
        FlushPoller { file }.await.map(|_| Errno::Success)
    })?))
}
