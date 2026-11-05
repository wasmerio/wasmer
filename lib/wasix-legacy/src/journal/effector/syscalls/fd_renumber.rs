use super::*;

impl JournalEffector {
    pub fn save_fd_renumber(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        from: Fd,
        to: Fd,
    ) -> anyhow::Result<()> {
        Self::save_event(
            ctx,
            JournalEntry::RenumberFileDescriptorV1 {
                old_fd: from,
                new_fd: to,
            },
        )
    }

    pub fn apply_fd_renumber(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        from: Fd,
        to: Fd,
    ) -> anyhow::Result<()> {
        let ret = crate::syscalls::fd_renumber_internal(ctx, from, to);
        if !matches!(ret, Ok(Errno::Success)) {
            bail!(
                "journal restore error: failed to renumber descriptor (from={}, to={}) - {}",
                from,
                to,
                ret.unwrap_or(Errno::Unknown)
            );
        }
        Ok(())
    }
}
