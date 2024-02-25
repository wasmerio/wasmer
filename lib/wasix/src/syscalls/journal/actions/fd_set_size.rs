use super::*;

impl<'a, 'c> JournalSyscallPlayer<'a, 'c> {
    pub(crate) unsafe fn action_fd_set_size(
        &mut self,
        fd: Fd,
        st_size: Filesize,
    ) -> Result<(), WasiRuntimeError> {
        tracing::trace!(%fd, %st_size, "Replay journal - FdSetSize");
        JournalEffector::apply_fd_set_size(&mut self.ctx, fd, st_size)
            .map_err(anyhow_err_to_runtime_err)?;
        Ok(())
    }
}
