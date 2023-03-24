use super::*;
use crate::syscalls::*;

/// ### `sock_leave_multicast_v6()`
/// Leaves a particular multicast IPv6 group
///
/// ## Parameters
///
/// * `fd` - Socket descriptor
/// * `multiaddr` - Multicast group to leave
/// * `interface` - Interface that will left
#[instrument(level = "debug", skip_all, fields(%sock, %iface), ret)]
pub fn sock_leave_multicast_v6<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    multiaddr: WasmPtr<__wasi_addr_ip6_t, M>,
    iface: u32,
) -> Errno {
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let multiaddr = wasi_try!(crate::net::read_ip_v6(&memory, multiaddr));
    wasi_try!(__sock_actor_mut(
        &mut ctx,
        sock,
        Rights::empty(),
        |mut socket, _| socket.leave_multicast_v6(multiaddr, iface)
    ));
    Errno::Success
}
