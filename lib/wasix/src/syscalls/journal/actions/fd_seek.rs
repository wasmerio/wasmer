use super::*;

impl<'a, 'c> JournalSyscallPlayer<'a, 'c> {
    #[allow(clippy::result_large_err)]
    pub(crate) unsafe fn action_fd_seek(
        &mut self,
        fd: u32,
        offset: i64,
        whence: Whence,
    ) -> Result<(), WasiRuntimeError> {
        tracing::trace!(%fd, %offset, ?whence, "Replay journal - FdSeek");
        JournalEffector::apply_fd_seek(&mut self.ctx, fd, offset, whence)
            .map_err(anyhow_err_to_runtime_err)?;
        Ok(())
    }
}
