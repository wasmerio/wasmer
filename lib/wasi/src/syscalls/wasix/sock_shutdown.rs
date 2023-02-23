use super::*;
use crate::syscalls::*;

/// ### `sock_shutdown()`
/// Shut down socket send and receive channels.
/// Note: This is similar to `shutdown` in POSIX.
///
/// ## Parameters
///
/// * `how` - Which channels on the socket to shut down.
pub fn sock_shutdown(mut ctx: FunctionEnvMut<'_, WasiEnv>, sock: WasiFd, how: SdFlags) -> Errno {
    debug!(
        "wasi[{}:{}]::sock_shutdown (fd={})",
        ctx.data().pid(),
        ctx.data().tid(),
        sock
    );

    let both = __WASI_SHUT_RD | __WASI_SHUT_WR;
    let how = match how {
        __WASI_SHUT_RD => std::net::Shutdown::Read,
        __WASI_SHUT_WR => std::net::Shutdown::Write,
        a if a == both => std::net::Shutdown::Both,
        _ => return Errno::Inval,
    };

    wasi_try!(__sock_actor_mut(
        &mut ctx,
        sock,
        Rights::SOCK_SHUTDOWN,
        |mut socket, _| socket.shutdown(how)
    ));

    Errno::Success
}
