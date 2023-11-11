use super::*;

impl SnapshotEffector {
    pub fn save_fd_renumber(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        from: Fd,
        to: Fd,
    ) -> anyhow::Result<()> {
        Self::save_event(
            ctx,
            SnapshotLog::RenumberFileDescriptor {
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
        if ret != Errno::Success {
            bail!(
                "snapshot restore error: failed to renumber descriptor (from={}, to={}) - {}",
                from,
                to,
                ret
            );
        }
        Ok(())
    }
}
