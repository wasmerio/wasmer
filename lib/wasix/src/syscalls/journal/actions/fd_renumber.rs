use super::*;

impl<'a, 'c> JournalSyscallPlayer<'a, 'c> {
    #[allow(clippy::result_large_err)]
    pub(crate) unsafe fn action_fd_renumber(
        &mut self,
        old_fd: u32,
        new_fd: u32,
    ) -> Result<(), WasiRuntimeError> {
        tracing::trace!(%old_fd, %new_fd, "Replay journal - FdRenumber");
        self.real_fd.insert(new_fd);
        if old_fd != new_fd {
            self.stdout_fds.remove(&new_fd);
            self.stderr_fds.remove(&new_fd);
        }
        if self.stdout_fds.remove(&old_fd) {
            self.stdout_fds.insert(new_fd);
        }
        if self.stderr_fds.remove(&old_fd) {
            self.stderr_fds.insert(new_fd);
        }
        JournalEffector::apply_fd_renumber(&mut self.ctx, old_fd, new_fd)
            .map_err(anyhow_err_to_runtime_err)?;
        Ok(())
    }
}
