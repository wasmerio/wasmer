use super::*;

impl<'a, 'c> JournalSyscallPlayer<'a, 'c> {
    #[allow(clippy::result_large_err)]
    pub(crate) unsafe fn action_fd_allocate(
        &mut self,
        fd: Fd,
        offset: Filesize,
        len: Filesize,
    ) -> Result<(), WasiRuntimeError> {
        tracing::trace!(%fd, %offset, %len, "Replay journal - FdAllocate");
        JournalEffector::apply_fd_allocate(&mut self.ctx, fd, offset, len)
            .map_err(anyhow_err_to_runtime_err)?;
        Ok(())
    }
}
