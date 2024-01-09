use std::net::Ipv4Addr;

use super::*;

impl JournalEffector {
    pub fn save_sock_leave_ipv4_multicast(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        multiaddr: Ipv4Addr,
        iface: Ipv4Addr,
    ) -> anyhow::Result<()> {
        Self::save_event(
            ctx,
            JournalEntry::SocketLeaveIpv4MulticastV1 {
                fd,
                multi_addr: multiaddr,
                iface,
            },
        )
    }

    pub fn apply_sock_leave_ipv4_multicast(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        multiaddr: Ipv4Addr,
        iface: Ipv4Addr,
    ) -> anyhow::Result<()> {
        crate::syscalls::sock_leave_multicast_v4_internal(ctx, fd, multiaddr, iface)
            .map(|r| r.map_err(|err| err.to_string()))
            .unwrap_or_else(|err| Err(err.to_string()))
            .map_err(|err| {
                anyhow::format_err!(
                    "journal restore error: failed to leave ipv4 multicast (fd={}, multiaddr={}, iface={}) - {}",
                    fd,
                    multiaddr,
                    iface,
                    err
                )
            })?;
        Ok(())
    }
}
