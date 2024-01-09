use std::net::Ipv4Addr;

use super::*;

impl JournalEffector {
    pub fn save_sock_join_ipv4_multicast(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        multiaddr: Ipv4Addr,
        iface: Ipv4Addr,
    ) -> anyhow::Result<()> {
        Self::save_event(
            ctx,
            JournalEntry::SocketJoinIpv4MulticastV1 {
                fd,
                multiaddr,
                iface,
            },
        )
    }

    pub fn apply_sock_join_ipv4_multicast(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        multiaddr: Ipv4Addr,
        iface: Ipv4Addr,
    ) -> anyhow::Result<()> {
        crate::syscalls::sock_join_multicast_v4_internal(ctx, fd, multiaddr, iface)
            .map(|r| r.map_err(|err| err.to_string()))
            .unwrap_or_else(|err| Err(err.to_string()))
            .map_err(|err| {
                anyhow::format_err!(
                    "journal restore error: failed to join ipv4 multicast (fd={}, multiaddr={:?}, iface={:?}) - {}",
                    fd,
                    multiaddr,
                    iface,
                    err
                )
            })?;
        Ok(())
    }
}
