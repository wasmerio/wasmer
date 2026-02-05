use super::*;
use crate::syscalls::*;
use vfs_core::flags::HandleStatusFlags;
use vfs_unix::errno::vfs_error_to_wasi_errno;

/// ### `fd_fdstat_set_flags()`
/// Set file descriptor flags for a file descriptor
/// Inputs:
/// - `Fd fd`
///     The file descriptor to apply the new flags to
/// - `Fdflags flags`
///     The flags to apply to `fd`
#[instrument(level = "trace", skip_all, fields(%fd), ret)]
pub fn fd_fdstat_set_flags(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    flags: Fdflags,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let ret = fd_fdstat_set_flags_internal(&mut ctx, fd, flags)?;
    let env = ctx.data();

    if ret == Errno::Success {
        #[cfg(feature = "journal")]
        if env.enable_journal {
            JournalEffector::save_fd_set_flags(&mut ctx, fd, flags).map_err(|err| {
                tracing::error!("failed to save file set flags event - {}", err);
                WasiError::Exit(ExitCode::from(Errno::Fault))
            })?;
        }
    }

    Ok(ret)
}

pub(crate) fn fd_fdstat_set_flags_internal(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    flags: Fdflags,
) -> Result<Errno, WasiError> {
    let env = ctx.data();
    let (_, mut state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };
    let mut fd_map = state.fs.fd_map.write().unwrap();
    let fd_entry = wasi_try_ok!(fd_map.get_entry_mut(fd).ok_or(Errno::Badf));
    if !fd_entry.inner.rights.contains(Rights::FD_FDSTAT_SET_FLAGS) {
        return Ok(Errno::Access);
    }
    fd_entry.inner.flags = flags;

    let mut status_flags = HandleStatusFlags::empty();
    status_flags.set(HandleStatusFlags::APPEND, flags.contains(Fdflags::APPEND));
    status_flags.set(
        HandleStatusFlags::NONBLOCK,
        flags.contains(Fdflags::NONBLOCK),
    );
    status_flags.set(HandleStatusFlags::SYNC, flags.contains(Fdflags::SYNC));
    status_flags.set(HandleStatusFlags::DSYNC, flags.contains(Fdflags::DSYNC));

    if let Kind::VfsFile { handle } = &fd_entry.kind {
        if let Err(err) = handle.set_status_flags(status_flags) {
            return Ok(vfs_error_to_wasi_errno(&err));
        }
    }
    Ok(Errno::Success)
}
