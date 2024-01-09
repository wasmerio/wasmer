use super::*;
use crate::syscalls::*;

/// ### `sock_set_opt_flag()`
/// Sets a particular socket setting
/// Note: This is similar to `setsockopt` in POSIX for SO_REUSEADDR
///
/// ## Parameters
///
/// * `fd` - Socket descriptor
/// * `sockopt` - Socket option to be set
/// * `flag` - Value to set the option to
#[instrument(level = "debug", skip_all, fields(%sock, %opt, %flag), ret)]
pub fn sock_set_opt_flag(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    opt: Sockoption,
    flag: Bool,
) -> Result<Errno, WasiError> {
    let flag = match flag {
        Bool::False => false,
        Bool::True => true,
        _ => return Ok(Errno::Inval),
    };

    wasi_try_ok!(sock_set_opt_flag_internal(&mut ctx, sock, opt, flag)?);

    #[cfg(feature = "journal")]
    if ctx.data().enable_journal {
        JournalEffector::save_sock_set_opt_flag(&mut ctx, sock, opt, flag).map_err(|err| {
            tracing::error!("failed to save sock_set_opt_flag event - {}", err);
            WasiError::Exit(ExitCode::Errno(Errno::Fault))
        })?;
    }

    Ok(Errno::Success)
}

pub(crate) fn sock_set_opt_flag_internal(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    opt: Sockoption,
    flag: bool,
) -> Result<Result<(), Errno>, WasiError> {
    let option: crate::net::socket::WasiSocketOption = opt.into();
    wasi_try_ok_ok!(__sock_actor_mut(
        ctx,
        sock,
        Rights::empty(),
        |mut socket, _| socket.set_opt_flag(option, flag)
    ));
    Ok(Ok(()))
}
