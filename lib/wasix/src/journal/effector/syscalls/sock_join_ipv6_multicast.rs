use std::net::Ipv6Addr;

use super::*;

impl JournalEffector {
    pub fn save_sock_join_ipv6_multicast(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        multiaddr: Ipv6Addr,
        iface: u32,
    ) -> anyhow::Result<()> {
        Self::save_event(
            ctx,
            JournalEntry::SocketJoinIpv6MulticastV1 {
                fd,
                multi_addr: multiaddr,
                iface,
            },
        )
    }

    pub fn apply_sock_join_ipv6_multicast(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        multiaddr: Ipv6Addr,
        iface: u32,
    ) -> anyhow::Result<()> {
        crate::syscalls::sock_join_multicast_v6_internal(ctx, fd, multiaddr, iface)
            .map(|r| r.map_err(|err| err.to_string()))
            .unwrap_or_else(|err| Err(err.to_string()))
            .map_err(|err| {
                anyhow::format_err!(
                    "journal restore error: failed to join ipv6 multicast (fd={fd}, multiaddr={multiaddr}, iface={iface}) - {err}")
            })?;
        Ok(())
    }
}
