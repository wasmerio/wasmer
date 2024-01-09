use super::*;

impl JournalEffector {
    pub fn save_fd_seek(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        offset: i64,
        whence: Whence,
    ) -> anyhow::Result<()> {
        Self::save_event(
            ctx,
            JournalEntry::FileDescriptorSeekV1 { fd, offset, whence },
        )
    }

    pub fn apply_fd_seek(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        offset: i64,
        whence: Whence,
    ) -> anyhow::Result<()> {
        crate::syscalls::fd_seek_internal(ctx, fd, offset, whence)?.map_err(|err| {
            anyhow::format_err!(
                "journal restore error: failed to seek (fd={}, offset={}, whence={:?}) - {}",
                fd,
                offset,
                whence,
                err
            )
        })?;
        Ok(())
    }
}
