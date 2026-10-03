use super::*;
use crate::fs::ShutdownPoller;
use crate::syscalls::*;

/// Best-effort close-time drain of the final file handle returned by fd removal.
pub(crate) fn shutdown_captured_handle(
    env: &WasiEnv,
    shutdown_target: Option<
        std::sync::Arc<std::sync::RwLock<Box<dyn virtual_fs::VirtualFile + Send + Sync>>>,
    >,
) -> Result<Errno, WasiError> {
    let Some(file) = shutdown_target else {
        return Ok(Errno::Success);
    };

    match __asyncify_light(env, None, ShutdownPoller { file })? {
        Ok(_)
        | Err(Errno::Isdir)
        | Err(Errno::Io)
        | Err(Errno::Access)
        // EINVAL is returned by e.g. pipe-backed stdio and is safe to ignore.
        | Err(Errno::Inval) => Ok(Errno::Success),
        Err(e) => Ok(e),
    }
}

/// ### `fd_close()`
/// Close an open file descriptor
/// For sockets this will flush the data before the socket is closed
/// Inputs:
/// - `Fd fd`
///     A file descriptor mapping to an open file to close
/// Errors:
/// - `Errno::Isdir`
///     If `fd` is a directory
/// - `Errno::Badf`
///     If `fd` is invalid or not open
#[instrument(level = "trace", skip_all, fields(pid = ctx.data().process.pid().raw(), %fd), ret)]
pub fn fd_close(mut ctx: FunctionEnvMut<'_, WasiEnv>, fd: WasiFd) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let env = ctx.data();
    let (_, mut state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };

    let outcome = state.fs.close_fd_and_capture_shutdown(fd);

    if outcome.skipped_preopen {
        trace!("Skipping fd_close for pre-opened FD ({})", fd);
        return Ok(Errno::Success);
    }

    if !outcome.removed {
        return Ok(Errno::Badf);
    }

    shutdown_captured_handle(env, outcome.shutdown_target)?;

    #[cfg(feature = "journal")]
    if env.enable_journal {
        JournalEffector::save_fd_close(&mut ctx, fd).map_err(|err| {
            tracing::error!("failed to save close descriptor event - {}", err);
            WasiError::Exit(ExitCode::from(Errno::Fault))
        })?;
    }

    Ok(Errno::Success)
}
