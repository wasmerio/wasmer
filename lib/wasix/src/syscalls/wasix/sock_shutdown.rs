use std::net::Shutdown;

use super::*;
use crate::syscalls::*;

/// ### `sock_shutdown()`
/// Shut down socket send and receive channels.
/// Note: This is similar to `shutdown` in POSIX.
///
/// ## Parameters
///
/// * `how` - Which channels on the socket to shut down.
#[instrument(level = "trace", skip_all, fields(%sock), ret)]
pub fn sock_shutdown(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    how: SdFlags,
) -> Result<Errno, WasiError> {
    let both = __WASI_SHUT_RD | __WASI_SHUT_WR;
    let shutdown = match how {
        __WASI_SHUT_RD => std::net::Shutdown::Read,
        __WASI_SHUT_WR => std::net::Shutdown::Write,
        a if a == both => std::net::Shutdown::Both,
        _ => return Ok(Errno::Inval),
    };

    wasi_try_ok!(sock_shutdown_internal(&mut ctx, sock, shutdown)?);

    #[cfg(feature = "journal")]
    if ctx.data().enable_journal {
        JournalEffector::save_sock_shutdown(&mut ctx, sock, shutdown).map_err(|err| {
            tracing::error!("failed to save sock_shutdown event - {}", err);
            WasiError::Exit(ExitCode::from(Errno::Fault))
        })?;
    }

    Ok(Errno::Success)
}

pub(crate) fn sock_shutdown_internal(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    shutdown: Shutdown,
) -> Result<Result<(), Errno>, WasiError> {
    wasi_try_ok_ok!(__sock_actor_mut(
        ctx,
        sock,
        Rights::SOCK_SHUTDOWN,
        |mut socket, _| socket.shutdown(shutdown)
    ));

    Ok(Ok(()))
}
