use std::net::SocketAddr;

use super::*;

impl JournalEffector {
    pub fn save_sock_bind(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        addr: SocketAddr,
    ) -> anyhow::Result<()> {
        Self::save_event(ctx, JournalEntry::SocketBindV1 { fd, addr })
    }

    pub fn apply_sock_bind(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        addr: SocketAddr,
    ) -> anyhow::Result<()> {
        crate::syscalls::sock_bind_internal(ctx, fd, addr)
            .map(|r| r.map_err(|err| err.to_string()))
            .unwrap_or_else(|err| Err(err.to_string()))
            .map_err(|err| {
                anyhow::format_err!(
                    "journal restore error: failed to bind socket to address (fd={}, addr={}) - {}",
                    fd,
                    addr,
                    err
                )
            })?;
        Ok(())
    }
}
