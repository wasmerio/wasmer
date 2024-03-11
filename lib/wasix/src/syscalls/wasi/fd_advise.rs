use super::*;
use crate::syscalls::*;

/// ### `fd_advise()`
/// Advise the system about how a file will be used
/// Inputs:
/// - `Fd fd`
///     The file descriptor the advice applies to
/// - `Filesize offset`
///     The offset from which the advice applies
/// - `Filesize len`
///     The length from the offset to which the advice applies
/// - `__wasi_advice_t advice`
///     The advice to give
#[instrument(level = "debug", skip_all, fields(%fd, %offset, %len, ?advice), ret)]
pub fn fd_advise(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    offset: Filesize,
    len: Filesize,
    advice: Advice,
) -> Result<Errno, WasiError> {
    wasi_try_ok!(fd_advise_internal(&mut ctx, fd, offset, len, advice));
    let env = ctx.data();

    #[cfg(feature = "journal")]
    if env.enable_journal {
        JournalEffector::save_fd_advise(&mut ctx, fd, offset, len, advice).map_err(|err| {
            tracing::error!("failed to save file descriptor advise event - {}", err);
            WasiError::Exit(ExitCode::Errno(Errno::Fault))
        })?;
    }

    Ok(Errno::Success)
}

pub(crate) fn fd_advise_internal(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    offset: Filesize,
    len: Filesize,
    advice: Advice,
) -> Result<(), Errno> {
    // this is used for our own benefit, so just returning success is a valid
    // implementation for now
    Ok(())
}
