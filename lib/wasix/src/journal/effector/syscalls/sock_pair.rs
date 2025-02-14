use super::*;

impl JournalEffector {
    // Note: since the current implementation uses a pipe, we don't store the
    // socket properties (domain, address family, etc.) in the journal.
    // Once the sock_pair syscall is fixed, we should create a SocketPairV2
    // entry that stores the socket properties as well. This ensures
    // forward-compatibility when that change is implemented.
    pub fn save_sock_pair(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd1: Fd,
        fd2: Fd,
    ) -> anyhow::Result<()> {
        Self::save_event(ctx, JournalEntry::SocketPairV1 { fd1, fd2 })
    }

    pub fn apply_sock_pair(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd1: Fd,
        fd2: Fd,
    ) -> anyhow::Result<()> {
        crate::syscalls::sock_pair_internal(ctx, Some(fd1), Some(fd2)).map_err(|err| {
            anyhow::format_err!(
                "journal restore error: failed to create socket pair - {}",
                err
            )
        })?;
        Ok(())
    }
}
