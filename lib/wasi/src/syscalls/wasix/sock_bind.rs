use super::*;
use crate::syscalls::*;

/// ### `sock_bind()`
/// Bind a socket
/// Note: This is similar to `bind` in POSIX using PF_INET
///
/// ## Parameters
///
/// * `fd` - File descriptor of the socket to be bind
/// * `addr` - Address to bind the socket to
pub fn sock_bind<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    addr: WasmPtr<__wasi_addr_port_t, M>,
) -> Errno {
    debug!(
        "wasi[{}:{}]::sock_bind (fd={})",
        ctx.data().pid(),
        ctx.data().tid(),
        sock
    );

    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let addr = wasi_try!(crate::net::read_ip_port(&memory, addr));
    let addr = SocketAddr::new(addr.0, addr.1);
    let net = env.net().clone();

    let tasks = ctx.data().tasks().clone();
    wasi_try!(__sock_upgrade(
        &mut ctx,
        sock,
        Rights::SOCK_BIND,
        move |socket| async move { socket.bind(tasks.deref(), net.deref(), addr).await }
    ));

    Errno::Success
}
