use super::*;

impl JournalEffector {
    pub fn save_fd_set_size(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        st_size: Filesize,
    ) -> anyhow::Result<()> {
        Self::save_event(ctx, JournalEntry::FileDescriptorSetSizeV1 { fd, st_size })
    }

    pub fn apply_fd_set_size(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        st_size: Filesize,
    ) -> anyhow::Result<()> {
        crate::syscalls::fd_filestat_set_size_internal(ctx, fd, st_size).map_err(|err| {
            anyhow::format_err!(
                "journal restore error: failed to set file size (fd={}, st_size={}) - {}",
                fd,
                st_size,
                err
            )
        })?;
        Ok(())
    }
}
