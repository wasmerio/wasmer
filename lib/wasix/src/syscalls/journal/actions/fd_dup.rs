use super::*;

impl<'a, 'c> JournalSyscallPlayer<'a, 'c> {
    #[allow(clippy::result_large_err)]
    pub(crate) unsafe fn action_fd_dup(
        &mut self,
        original_fd: u32,
        copied_fd: u32,
        cloexec: bool,
    ) -> Result<(), WasiRuntimeError> {
        tracing::trace!(%original_fd, %copied_fd, "Replay journal - FdDuplicate");
        self.real_fd.insert(copied_fd);
        if original_fd != copied_fd {
            self.stdout_fds.remove(&copied_fd);
            self.stderr_fds.remove(&copied_fd);
        }
        if self.stdout_fds.contains(&original_fd) {
            self.stdout_fds.insert(copied_fd);
        }
        if self.stderr_fds.contains(&original_fd) {
            self.stderr_fds.insert(copied_fd);
        }
        JournalEffector::apply_fd_duplicate(&mut self.ctx, original_fd, copied_fd, cloexec)
            .map_err(anyhow_err_to_runtime_err)?;
        Ok(())
    }
}
