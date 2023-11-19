use super::*;

impl SnapshotEffector {
    pub fn save_fd_duplicate(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        original_fd: Fd,
        copied_fd: Fd,
    ) -> anyhow::Result<()> {
        Self::save_event(
            ctx,
            SnapshotLog::DuplicateFileDescriptor {
                original_fd,
                copied_fd,
            },
        )
    }

    pub fn apply_fd_duplicate(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        original_fd: Fd,
        copied_fd: Fd,
    ) -> anyhow::Result<()> {
        let ret_fd = crate::syscalls::fd_dup_internal(ctx, original_fd)
            .map_err(|err| {
                anyhow::format_err!(
                    "snapshot restore error: failed to duplicate file descriptor (original={}, copied={}) - {}",
                    original_fd,
                    copied_fd,
                    err
                )    
            })?;

        let ret = crate::syscalls::fd_renumber_internal(ctx, ret_fd, copied_fd);
        if ret != Errno::Success {
            bail!(
                "snapshot restore error: failed renumber file descriptor after duplicate (from={}, to={}) - {}",
                ret_fd,
                copied_fd,
                ret
            );
        }
        Ok(())
    }
}
