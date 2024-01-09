use std::{net::IpAddr, time::Duration};
use virtual_net::IpCidr;

use super::*;

impl JournalEffector {
    pub fn save_port_route_add(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        cidr: IpCidr,
        via_router: IpAddr,
        preferred_until: Option<Duration>,
        expires_at: Option<Duration>,
    ) -> anyhow::Result<()> {
        Self::save_event(
            ctx,
            JournalEntry::PortRouteAddV1 {
                cidr,
                via_router,
                preferred_until,
                expires_at,
            },
        )
    }

    pub fn apply_port_route_add(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        cidr: IpCidr,
        via_router: IpAddr,
        preferred_until: Option<Duration>,
        expires_at: Option<Duration>,
    ) -> anyhow::Result<()> {
        crate::syscalls::port_route_add_internal(
            ctx,
            cidr,
            via_router,
            preferred_until,
            expires_at,
        )
        .map(|r| r.map_err(|err| err.to_string()))
        .unwrap_or_else(|err| Err(err.to_string()))
        .map_err(|err| {
            anyhow::format_err!(
                "journal restore error: failed to add route (cidr={:?}, via_router={}, preferred_until={:?}, expires_at={:?}) - {}",
                cidr,
                via_router,
                preferred_until,
                expires_at,
                err
            )
        })?;
        Ok(())
    }
}
