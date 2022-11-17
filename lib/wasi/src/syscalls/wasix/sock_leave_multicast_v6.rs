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
pub fn sock_leave_multicast_v6<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    multiaddr: WasmPtr<__wasi_addr_ip6_t, M>,
    iface: u32,
) -> Errno {
    debug!(
        "wasi[{}:{}]::sock_leave_multicast_v6 (fd={})",
        ctx.data().pid(),
        ctx.data().tid(),
        sock
    );

    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let multiaddr = wasi_try!(crate::state::read_ip_v6(&memory, multiaddr));
    wasi_try!(__asyncify(&mut ctx, None, move |ctx| async move {
        __sock_actor_mut(
            ctx,
            sock,
            Rights::empty(),
            move |mut socket| async move { socket.leave_multicast_v6(multiaddr, iface).await }
        )
        .await
    }));
    Errno::Success
}
