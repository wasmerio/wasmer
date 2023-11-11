use super::*;

impl SnapshotEffector {
    pub fn save_fd_advise(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        offset: Filesize,
        len: Filesize,
        advice: Advice,
    ) -> anyhow::Result<()> {
        Self::save_event(
            ctx,
            SnapshotLog::FileDescriptorAdvise {
                fd,
                offset,
                len,
                advice: advice.into(),
            },
        )
    }

    pub fn apply_fd_advise(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        offset: Filesize,
        len: Filesize,
        advice: Advice,
    ) -> anyhow::Result<()> {
        crate::syscalls::fd_advise_internal(ctx, fd, offset, len, advice).map_err(|err| {
            anyhow::format_err!(
                "snapshot restore error: failed to advise file descriptor (fd={}, offset={}, len={}, advice={:?}) - {}",
                fd,
                offset,
                len,
                advice,
                err
            )
        })?;
        Ok(())
    }
}
