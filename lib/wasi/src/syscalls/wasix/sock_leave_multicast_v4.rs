use super::*;
use crate::syscalls::*;

/// ### `sock_leave_multicast_v4()`
/// Leaves a particular multicast IPv4 group
///
/// ## Parameters
///
/// * `fd` - Socket descriptor
/// * `multiaddr` - Multicast group to leave
/// * `interface` - Interface that will left
pub fn sock_leave_multicast_v4<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    multiaddr: WasmPtr<__wasi_addr_ip4_t, M>,
    iface: WasmPtr<__wasi_addr_ip4_t, M>,
) -> Errno {
    debug!(
        "wasi[{}:{}]::sock_leave_multicast_v4 (fd={})",
        ctx.data().pid(),
        ctx.data().tid(),
        sock
    );

    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let multiaddr = wasi_try!(crate::net::read_ip_v4(&memory, multiaddr));
    let iface = wasi_try!(crate::net::read_ip_v4(&memory, iface));
    wasi_try!(__sock_actor_mut(
        &mut ctx,
        sock,
        Rights::empty(),
        |socket, _| socket.leave_multicast_v4(multiaddr, iface)
    ));
    Errno::Success
}
