use super::*;

impl JournalEffector {
    pub fn save_fd_set_fdflags(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        flags: Fdflagsext,
    ) -> anyhow::Result<()> {
        Self::save_event(ctx, JournalEntry::FileDescriptorSetFdFlagsV1 { fd, flags })
    }

    pub fn apply_fd_set_fdflags(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        flags: Fdflagsext,
    ) -> anyhow::Result<()> {
        crate::syscalls::fd_fdflags_set_internal(ctx, fd, flags).map_err(|err| {
            anyhow::format_err!(
                "journal restore error: failed to set file flags (fd={}, flags={:?}) - {}",
                fd,
                flags,
                err
            )
        })?;
        Ok(())
    }
}
