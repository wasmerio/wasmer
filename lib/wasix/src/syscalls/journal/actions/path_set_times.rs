use super::*;

impl<'a, 'c> JournalSyscallPlayer<'a, 'c> {
    #[allow(clippy::result_large_err)]
    pub(crate) unsafe fn action_path_set_times(
        &mut self,
        fd: Fd,
        flags: LookupFlags,
        path: Cow<'_, str>,
        st_atim: Timestamp,
        st_mtim: Timestamp,
        fst_flags: Fstflags,
    ) -> Result<(), WasiRuntimeError> {
        tracing::trace!(%fd, "Replay journal - PathSetTimes");
        JournalEffector::apply_path_set_times(
            &mut self.ctx,
            fd,
            flags,
            &path,
            st_atim,
            st_mtim,
            fst_flags,
        )
        .map_err(anyhow_err_to_runtime_err)?;
        Ok(())
    }
}
