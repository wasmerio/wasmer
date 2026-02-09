use super::*;
use crate::fs::Kind;
use wasmer_wasix_types::wasi::Filetype;
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
    let state = env.state.clone();
    let fd_entry = wasi_try_ok!(state.fs.get_fd(fd));
    let is_dir = {
        let guard = fd_entry.inode.read();
        matches!(&*guard, Kind::Dir { .. } | Kind::Root { .. })
    };
    if is_dir {
        return Ok(Errno::Success);
    }
    let file_type = {
        let guard = fd_entry.inode.stat.read().unwrap();
        guard.st_filetype
    };
    if matches!(file_type, Filetype::CharacterDevice | Filetype::BlockDevice) {
        return Ok(Errno::Inval);
    }
    if !fd_entry.inner.rights.contains(Rights::FD_DATASYNC) {
        return Ok(Errno::Access);
    }

    #[allow(clippy::await_holding_lock)]
    Ok(wasi_try_ok!(__asyncify(&mut ctx, None, async move {
        state.fs.flush(fd).await.map(|_| Errno::Success)
    })?))
}
