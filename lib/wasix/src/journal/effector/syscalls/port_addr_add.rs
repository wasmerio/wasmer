use virtual_net::IpCidr;

use super::*;

impl JournalEffector {
    pub fn save_port_addr_add(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        cidr: IpCidr,
    ) -> anyhow::Result<()> {
        Self::save_event(ctx, JournalEntry::PortAddAddrV1 { cidr })
    }

    pub fn apply_port_addr_add(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        cidr: IpCidr,
    ) -> anyhow::Result<()> {
        crate::syscalls::port_addr_add_internal(ctx, cidr)
            .map(|r| r.map_err(|err| err.to_string()))
            .unwrap_or_else(|err| Err(err.to_string()))
            .map_err(|err| {
                anyhow::format_err!(
                    "journal restore error: failed to add address to port file descriptor (cidr={:?}) - {}",
                    cidr,
                    err
                )
            })?;
        Ok(())
    }
}
