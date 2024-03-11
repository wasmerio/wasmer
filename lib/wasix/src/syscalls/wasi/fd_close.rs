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
#[instrument(level = "debug", skip_all, fields(pid = ctx.data().process.pid().raw(), %fd), ret)]
pub fn fd_close(mut ctx: FunctionEnvMut<'_, WasiEnv>, fd: WasiFd) -> Result<Errno, WasiError> {
    wasi_try_ok!(WasiEnv::process_signals_and_exit(&mut ctx)?);

    // HACK: As a special case, we don't want to close fd 3 because it can break
    // some programs...
    // - On Wasix, we have some default fd, 0:stdin, 1:stdout, 2:stderr, 3:root
    // - On Posix, we have some default fd: 0:stdin, 1:stdout, 2:stderr
    // - A POSIX program might want to closed all open fd (e.g. when Python
    //   spawns subprocesses), so it blindly does fd_close() for fd=3..255
    // - Wasix doesn't work well when 3:root is closed, because it's the root
    if fd == 3 {
        tracing::debug!("Skipping fd_close(3)");
        return Ok(Errno::Success);
    }

    let env = ctx.data();
    let (_, mut state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };
    wasi_try_ok!(state.fs.close_fd(fd));

    #[cfg(feature = "journal")]
    if env.enable_journal {
        JournalEffector::save_fd_close(&mut ctx, fd).map_err(|err| {
            tracing::error!("failed to save close descriptor event - {}", err);
            WasiError::Exit(ExitCode::Errno(Errno::Fault))
        })?;
    }

    Ok(Errno::Success)
}
