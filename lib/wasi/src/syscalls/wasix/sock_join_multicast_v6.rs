use super::*;
use crate::syscalls::*;

/// ### `sock_join_multicast_v6()`
/// Joins a particular multicast IPv6 group
///
/// ## Parameters
///
/// * `fd` - Socket descriptor
/// * `multiaddr` - Multicast group to joined
/// * `interface` - Interface that will join
pub fn sock_join_multicast_v6<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    multiaddr: WasmPtr<__wasi_addr_ip6_t, M>,
    iface: u32,
) -> Result<Errno, WasiError> {
    debug!(
        "wasi[{}:{}]::sock_join_multicast_v6 (fd={})",
        ctx.data().pid(),
        ctx.data().tid(),
        sock
    );

    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let multiaddr = wasi_try_ok!(crate::net::read_ip_v6(&memory, multiaddr));
    wasi_try_ok!(__sock_actor_mut(
        &mut ctx,
        sock,
        Rights::empty(),
        move |socket| async move { socket.join_multicast_v6(multiaddr, iface).await }
    )?);
    Ok(Errno::Success)
}
