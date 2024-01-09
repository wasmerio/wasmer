use super::*;

impl JournalEffector {
    pub fn save_port_dhcp_acquire(ctx: &mut FunctionEnvMut<'_, WasiEnv>) -> anyhow::Result<()> {
        Self::save_event(ctx, JournalEntry::PortDhcpAcquireV1)
    }

    pub fn apply_port_dhcp_acquire(ctx: &mut FunctionEnvMut<'_, WasiEnv>) -> anyhow::Result<()> {
        crate::syscalls::port_dhcp_acquire_internal(ctx)
            .map(|r| r.map_err(|err| err.to_string()))
            .unwrap_or_else(|err| Err(err.to_string()))
            .map_err(|err| {
                anyhow::format_err!(
                    "journal restore error: failed to acquire DHCP address - {}",
                    err
                )
            })?;
        Ok(())
    }
}
