use super::*;

impl JournalEffector {
    pub fn save_fd_set_flags(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        flags: Fdflags,
    ) -> anyhow::Result<()> {
        Self::save_event(ctx, JournalEntry::FileDescriptorSetFlags { fd, flags })
    }

    pub fn apply_fd_set_flags(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        flags: Fdflags,
    ) -> anyhow::Result<()> {
        crate::syscalls::fd_fdstat_set_flags_internal(ctx, fd, flags)
            .map_err(|err| {
                anyhow::format_err!(
                    "snapshot restore error: failed to duplicate file descriptor (fd={}, flags={:?}) - {}",
                    fd,
                    flags,
                    err
                )
            })?;
        Ok(())
    }
}
