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
) -> Result<Errno, WasiError> {
    let env = ctx.data();
    let memory = unsafe { env.memory_view(&ctx) };
    let multiaddr = wasi_try_ok!(crate::net::read_ip_v6(&memory, multiaddr));

    wasi_try_ok!(sock_leave_multicast_v6_internal(
        &mut ctx, sock, multiaddr, iface
    )?);

    #[cfg(feature = "journal")]
    if ctx.data().enable_journal {
        JournalEffector::save_sock_leave_ipv6_multicast(&mut ctx, sock, multiaddr, iface).map_err(
            |err| {
                tracing::error!("failed to save sock_leave_ipv6_multicast event - {}", err);
                WasiError::Exit(ExitCode::Errno(Errno::Fault))
            },
        )?;
    }

    Ok(Errno::Success)
}

pub(crate) fn sock_leave_multicast_v6_internal(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    multiaddr: Ipv6Addr,
    iface: u32,
) -> Result<Result<(), Errno>, WasiError> {
    let env = ctx.data();
    wasi_try_ok_ok!(__sock_actor_mut(
        ctx,
        sock,
        Rights::empty(),
        |mut socket, _| socket.leave_multicast_v6(multiaddr, iface)
    ));
    Ok(Ok(()))
}
