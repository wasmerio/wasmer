use super::*;

impl JournalEffector {
    pub fn save_fd_set_times(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        st_atim: Timestamp,
        st_mtim: Timestamp,
        fst_flags: Fstflags,
    ) -> anyhow::Result<()> {
        Self::save_event(
            ctx,
            JournalEntry::FileDescriptorSetTimesV1 {
                fd,
                st_atim,
                st_mtim,
                fst_flags,
            },
        )
    }

    pub fn apply_fd_set_times(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        st_atim: Timestamp,
        st_mtim: Timestamp,
        fst_flags: Fstflags,
    ) -> anyhow::Result<()> {
        crate::syscalls::fd_filestat_set_times_internal(ctx, fd, st_atim, st_mtim, fst_flags)
            .map_err(|err| {
                anyhow::format_err!(
                    "journal restore error: failed to set file times (fd={}, st_atim={}, st_mtim={}, fst_flags={:?}) - {}",
                    fd,
                    st_atim,
                    st_mtim,
                    fst_flags,
                    err
                )
            })?;
        Ok(())
    }
}
