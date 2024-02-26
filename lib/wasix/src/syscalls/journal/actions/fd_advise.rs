use super::*;

impl<'a, 'c> JournalSyscallPlayer<'a, 'c> {
    #[allow(clippy::result_large_err)]
    pub(crate) unsafe fn action_fd_advise(
        &mut self,
        fd: Fd,
        offset: Filesize,
        len: Filesize,
        advice: Advice,
    ) -> Result<(), WasiRuntimeError> {
        tracing::trace!(%fd, %offset, %len, ?advice, "Replay journal - FdAdvise");
        JournalEffector::apply_fd_advise(&mut self.ctx, fd, offset, len, advice)
            .map_err(anyhow_err_to_runtime_err)?;
        Ok(())
    }
}
