use std::net::Shutdown;

use super::*;

impl JournalEffector {
    pub fn save_sock_shutdown(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        shutdown: Shutdown,
    ) -> anyhow::Result<()> {
        Self::save_event(
            ctx,
            JournalEntry::SocketShutdownV1 {
                fd,
                how: shutdown.into(),
            },
        )
    }

    pub fn apply_sock_shutdown(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        shutdown: Shutdown,
    ) -> anyhow::Result<()> {
        crate::syscalls::sock_shutdown_internal(ctx, fd, shutdown)
            .map(|r| r.map_err(|err| err.to_string()))
            .unwrap_or_else(|err| Err(err.to_string()))
            .map_err(|err| {
                anyhow::format_err!(
                    "journal restore error: failed to shutdown socket (fd={}, how={:?}) - {}",
                    fd,
                    shutdown,
                    err
                )
            })?;
        Ok(())
    }
}
