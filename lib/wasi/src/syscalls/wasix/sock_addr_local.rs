use super::*;
use crate::syscalls::*;

/// ### `sock_addr_local()`
/// Returns the local address to which the socket is bound.
///
/// Note: This is similar to `getsockname` in POSIX
///
/// When successful, the contents of the output buffer consist of an IP address,
/// either IP4 or IP6.
///
/// ## Parameters
///
/// * `fd` - Socket that the address is bound to
pub fn sock_addr_local<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    ret_addr: WasmPtr<__wasi_addr_port_t, M>,
) -> Errno {
    debug!(
        "wasi[{}:{}]::sock_addr_local (fd={})",
        ctx.data().pid(),
        ctx.data().tid(),
        sock
    );

    let addr = wasi_try!(__sock_actor(
        &mut ctx,
        sock,
        Rights::empty(),
        |socket, _| socket.addr_local()
    ));
    let memory = ctx.data().memory_view(&ctx);
    wasi_try!(crate::net::write_ip_port(
        &memory,
        ret_addr,
        addr.ip(),
        addr.port()
    ));
    Errno::Success
}
