use super::*;

impl SnapshotEffector {
    pub fn save_fd_allocate(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        offset: Filesize,
        len: Filesize,
    ) -> anyhow::Result<()> {
        Self::save_event(ctx, SnapshotLog::FileDescriptorAllocate { fd, offset, len })
    }

    pub fn apply_fd_allocate(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        offset: Filesize,
        len: Filesize,
    ) -> anyhow::Result<()> {
        crate::syscalls::fd_allocate_internal(ctx, fd, offset, len)
            .map_err(|err| {
                anyhow::format_err!(
                    "snapshot restore error: failed to allocate on file descriptor (fd={}, offset={}, len={}) - {}",
                    fd,
                    offset,
                    len,
                    err
                )    
            })?;
        Ok(())
    }
}
