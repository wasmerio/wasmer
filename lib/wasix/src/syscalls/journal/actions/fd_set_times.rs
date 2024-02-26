use super::*;

impl<'a, 'c> JournalSyscallPlayer<'a, 'c> {
    #[allow(clippy::result_large_err)]
    pub(crate) unsafe fn action_fd_set_times(
        &mut self,
        fd: Fd,
        st_atim: Timestamp,
        st_mtim: Timestamp,
        fst_flags: Fstflags,
    ) -> Result<(), WasiRuntimeError> {
        tracing::trace!(%fd, %st_atim, %st_mtim, ?fst_flags, "Replay journal - FdSetTimes");
        JournalEffector::apply_fd_set_times(&mut self.ctx, fd, st_atim, st_mtim, fst_flags)
            .map_err(anyhow_err_to_runtime_err)?;
        Ok(())
    }
}
