use std::net::IpAddr;

use super::*;

impl JournalEffector {
    pub fn save_port_gateway_set(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        ip: IpAddr,
    ) -> anyhow::Result<()> {
        Self::save_event(ctx, JournalEntry::PortGatewaySetV1 { ip })
    }

    pub fn apply_port_gateway_set(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        ip: IpAddr,
    ) -> anyhow::Result<()> {
        crate::syscalls::port_gateway_set_internal(ctx, ip)
            .map(|r| r.map_err(|err| err.to_string()))
            .unwrap_or_else(|err| Err(err.to_string()))
            .map_err(|err| {
                anyhow::format_err!(
                    "journal restore error: failed to set gateway address (ip={}) - {}",
                    ip,
                    err
                )
            })?;
        Ok(())
    }
}
