use super::*;
use crate::syscalls::*;

/// ### `sock_addr_peer()`
/// Returns the remote address to which the socket is connected to.
///
/// Note: This is similar to `getpeername` in POSIX
///
/// When successful, the contents of the output buffer consist of an IP address,
/// either IP4 or IP6.
///
/// ## Parameters
///
/// * `fd` - Socket that the address is bound to
pub fn sock_addr_peer<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    ro_addr: WasmPtr<__wasi_addr_port_t, M>,
) -> Errno {
    debug!(
        "wasi[{}:{}]::sock_addr_peer (fd={})",
        ctx.data().pid(),
        ctx.data().tid(),
        sock
    );

    let addr = wasi_try!(__asyncify(&mut ctx, None, async move {
        __sock_actor(
            &mut ctx,
            sock,
            Rights::empty(),
            move |socket| async move { socket.addr_peer() }
        ).await
    }));

    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    wasi_try!(crate::state::write_ip_port(
        &memory,
        ro_addr,
        addr.ip(),
        addr.port()
    ));
    Errno::Success
}