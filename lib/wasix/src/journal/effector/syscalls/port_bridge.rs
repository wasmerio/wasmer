use virtual_net::StreamSecurity;

use super::*;

impl JournalEffector {
    pub fn save_port_bridge(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        network: String,
        token: String,
        security: StreamSecurity,
    ) -> anyhow::Result<()> {
        Self::save_event(
            ctx,
            JournalEntry::PortBridgeV1 {
                network: network.into(),
                token: token.into(),
                security,
            },
        )
    }

    pub fn apply_port_bridge(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        network: &str,
        token: &str,
        security: StreamSecurity,
    ) -> anyhow::Result<()> {
        crate::syscalls::port_bridge_internal(ctx, network, token, security)
            .map(|r| r.map_err(|err| err.to_string()))
            .unwrap_or_else(|err| Err(err.to_string()))
            .map_err(|err| {
                anyhow::format_err!(
                    "journal restore error: failed to bridge the network file descriptor (network={}, security={:?}) - {}",
                    network,
                    security,
                    err
                )
            })?;
        Ok(())
    }
}
