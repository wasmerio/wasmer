use super::*;

impl JournalEffector {
    pub fn save_sock_listen(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        backlog: usize,
    ) -> anyhow::Result<()> {
        Self::save_event(
            ctx,
            JournalEntry::SocketListenV1 {
                fd,
                backlog: backlog as u32,
            },
        )
    }

    pub fn apply_sock_listen(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        backlog: usize,
    ) -> anyhow::Result<()> {
        crate::syscalls::sock_listen_internal(ctx, fd, backlog)
            .map(|r| r.map_err(|err| err.to_string()))
            .unwrap_or_else(|err| Err(err.to_string()))
            .map_err(|err| {
                anyhow::format_err!(
                    "journal restore error: failed to listen on socket (fd={}, backlog={}) - {}",
                    fd,
                    backlog,
                    err
                )
            })?;
        Ok(())
    }
}
