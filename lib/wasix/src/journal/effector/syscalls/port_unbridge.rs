use super::*;

impl JournalEffector {
    pub fn save_port_unbridge(ctx: &mut FunctionEnvMut<'_, WasiEnv>) -> anyhow::Result<()> {
        Self::save_event(ctx, JournalEntry::PortUnbridgeV1)
    }

    pub fn apply_port_unbridge(ctx: &mut FunctionEnvMut<'_, WasiEnv>) -> anyhow::Result<()> {
        crate::syscalls::port_unbridge_internal(ctx)
            .map(|r| r.map_err(|err| err.to_string()))
            .unwrap_or_else(|err| Err(err.to_string()))
            .map_err(|err| {
                anyhow::format_err!(
                    "journal restore error: failed to un-bridge network - {}",
                    err
                )
            })?;
        Ok(())
    }
}
