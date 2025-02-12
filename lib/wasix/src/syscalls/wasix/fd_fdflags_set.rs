use super::*;
use crate::syscalls::*;

/// ### `fd_fdstat_set_flags()`
/// Set file descriptor flags for a file descriptor
/// Inputs:
/// - `Fd fd`
///     The file descriptor to apply the new flags to
/// - `Fdflags flags`
///     The flags to apply to `fd`
#[instrument(level = "trace", skip_all, fields(%fd), ret)]
pub fn fd_fdflags_set(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    flags: Fdflagsext,
) -> Result<Errno, WasiError> {
    let ret = fd_fdflags_set_internal(&mut ctx, fd, flags)?;

    if ret == Errno::Success {
        #[cfg(feature = "journal")]
        {
            let env = ctx.data();
            if env.enable_journal {
                JournalEffector::save_fd_set_fdflags(&mut ctx, fd, flags).map_err(|err| {
                    tracing::error!("failed to save file set fd flags event - {}", err);
                    WasiError::Exit(ExitCode::from(Errno::Fault))
                })?;
            }
        }
    }

    Ok(ret)
}

pub(crate) fn fd_fdflags_set_internal(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    flags: Fdflagsext,
) -> Result<Errno, WasiError> {
    let env = ctx.data();
    let (_, mut state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };
    let mut fd_map = state.fs.fd_map.write().unwrap();
    let mut fd_entry = wasi_try_ok!(fd_map.get_mut(fd).ok_or(Errno::Badf));
    if !fd_entry.rights.contains(Rights::FD_FDSTAT_SET_FLAGS) {
        return Ok(Errno::Access);
    }
    fd_entry.fd_flags = flags;
    Ok(Errno::Success)
}
