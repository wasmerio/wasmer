use super::*;
use crate::syscalls::*;

/// ### `sock_connect()`
/// Initiate a connection on a socket to the specified address
///
/// Polling the socket handle will wait for data to arrive or for
/// the socket status to change which can be queried via 'sock_status'
///
/// Note: This is similar to `connect` in POSIX
///
/// ## Parameters
///
/// * `fd` - Socket descriptor
/// * `addr` - Address of the socket to connect to
pub fn sock_connect<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    addr: WasmPtr<__wasi_addr_port_t, M>,
) -> Errno {
    debug!(
        "wasi[{}:{}]::sock_connect (fd={})",
        ctx.data().pid(),
        ctx.data().tid(),
        sock
    );

    let env = ctx.data();
    let net = env.net().clone();
    let memory = env.memory_view(&ctx);
    let addr = wasi_try!(crate::net::read_ip_port(&memory, addr));
    let addr = SocketAddr::new(addr.0, addr.1);

    let tasks = ctx.data().tasks().clone();
    wasi_try!(__sock_upgrade(
        &mut ctx,
        sock,
        Rights::SOCK_CONNECT,
        move |mut socket| async move { socket.connect(tasks.deref(), net.deref(), addr, None).await }
    ));

    Errno::Success
}
