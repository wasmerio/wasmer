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
pub fn sock_set_opt_flag(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    opt: Sockoption,
    flag: Bool,
) -> Errno {
    debug!(
        "wasi[{}:{}]::sock_set_opt_flag(fd={}, ty={}, flag={:?})",
        ctx.data().pid(),
        ctx.data().tid(),
        sock,
        opt,
        flag
    );

    let flag = match flag {
        Bool::False => false,
        Bool::True => true,
        _ => return Errno::Inval,
    };

    let option: crate::net::socket::WasiSocketOption = opt.into();
    wasi_try!(__sock_actor_mut(
        &mut ctx,
        sock,
        Rights::empty(),
        |mut socket, _| socket.set_opt_flag(option, flag)
    ));
    Errno::Success
}
