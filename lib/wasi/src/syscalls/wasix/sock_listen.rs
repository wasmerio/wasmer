use super::*;
use crate::syscalls::*;

/// ### `sock_listen()`
/// Listen for connections on a socket
///
/// Polling the socket handle will wait until a connection
/// attempt is made
///
/// Note: This is similar to `listen`
///
/// ## Parameters
///
/// * `fd` - File descriptor of the socket to be bind
/// * `backlog` - Maximum size of the queue for pending connections
pub fn sock_listen<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    backlog: M::Offset,
) -> Errno {
    debug!(
        "wasi[{}:{}]::sock_listen (fd={})",
        ctx.data().pid(),
        ctx.data().tid(),
        sock
    );

    let env = ctx.data();
    let net = env.net().clone();
    let backlog: usize = wasi_try!(backlog.try_into().map_err(|_| Errno::Inval));

    let tasks = ctx.data().tasks().clone();
    wasi_try!(__sock_upgrade(
        &mut ctx,
        sock,
        Rights::SOCK_LISTEN,
        |socket| async move { socket.listen(tasks.deref(), net.deref(), backlog).await }
    ));

    Errno::Success
}
