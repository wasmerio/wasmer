use std::net::IpAddr;

use super::*;

impl JournalEffector {
    pub fn save_port_route_remove(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        ip: IpAddr,
    ) -> anyhow::Result<()> {
        Self::save_event(ctx, JournalEntry::PortRouteDelV1 { ip })
    }

    pub fn apply_port_route_remove(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        ip: IpAddr,
    ) -> anyhow::Result<()> {
        crate::syscalls::port_route_remove_internal(ctx, ip)
            .map(|r| r.map_err(|err| err.to_string()))
            .unwrap_or_else(|err| Err(err.to_string()))
            .map_err(|err| {
                anyhow::format_err!(
                    "journal restore error: failed to remove route (ip={}) - {}",
                    ip,
                    err
                )
            })?;
        Ok(())
    }
}
