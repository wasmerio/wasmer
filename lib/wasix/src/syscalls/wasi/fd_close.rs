use super::*;
use crate::syscalls::*;

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
    wasi_try_ok!(WasiEnv::process_signals_and_exit(&mut ctx)?);

    let env = ctx.data();
    let (_, mut state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };

    // We don't want to allow programs that blindly close all FDs in a loop
    // to be able to close pre-opens, as that breaks wasix-libc in rather
    // spectacular fashion.
    if let Ok(pfd) = state.fs.get_fd(fd) {
        if !pfd.is_stdio && pfd.inode.is_preopened {
            trace!("Skipping fd_close for pre-opened FD ({})", fd);
            return Ok(Errno::Success);
        }
    }
    // HACK: we use tokio files to back WASI file handles. Since tokio
    // does writes in the background, it may miss writes if the file is
    // closed without flushing first. Hence, we flush once here.
    match __asyncify_light(env, None, state.fs.flush(fd))? {
        Ok(_) | Err(Errno::Io) | Err(Errno::Access) => {}
        Err(e) => {
            return Ok(e);
        }
    }
    wasi_try_ok!(state.fs.close_fd(fd));

    #[cfg(feature = "journal")]
    if env.enable_journal {
        JournalEffector::save_fd_close(&mut ctx, fd).map_err(|err| {
            tracing::error!("failed to save close descriptor event - {}", err);
            WasiError::Exit(ExitCode::from(Errno::Fault))
        })?;
    }

    Ok(Errno::Success)
}
