use super::*;

impl<'a, 'c> JournalSyscallPlayer<'a, 'c> {
    #[allow(clippy::result_large_err)]
    pub(crate) unsafe fn action_fd_close(&mut self, fd: u32) -> Result<(), WasiRuntimeError> {
        tracing::trace!(%fd, "Replay journal - FdClose");
        self.stdout_fds.remove(&fd);
        self.stderr_fds.remove(&fd);
        JournalEffector::apply_fd_close(&mut self.ctx, fd).map_err(anyhow_err_to_runtime_err)?;
        Ok(())
    }
}
