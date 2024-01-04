use std::net::IpAddr;

use super::*;

impl JournalEffector {
    pub fn save_port_addr_remove(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        addr: IpAddr,
    ) -> anyhow::Result<()> {
        Self::save_event(ctx, JournalEntry::PortDelAddrV1 { addr })
    }

    pub fn apply_port_addr_remove(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        addr: IpAddr,
    ) -> anyhow::Result<()> {
        crate::syscalls::port_addr_remove_internal(ctx, addr)
            .map(|r| r.map_err(|err| err.to_string()))
            .unwrap_or_else(|err| Err(err.to_string()))
            .map_err(|err| {
                anyhow::format_err!(
                    "journal restore error: failed to remove address from port (ip={}) - {}",
                    addr,
                    err
                )
            })?;
        Ok(())
    }
}
