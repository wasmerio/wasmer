use super::*;

impl<'a, 'c> JournalSyscallPlayer<'a, 'c> {
    #[allow(clippy::result_large_err)]
    pub(crate) unsafe fn action_fd_set_rights(
        &mut self,
        fd: Fd,
        fs_rights_base: Rights,
        fs_rights_inheriting: Rights,
    ) -> Result<(), WasiRuntimeError> {
        tracing::trace!(%fd, "Replay journal - FdSetRights");
        JournalEffector::apply_fd_set_rights(
            &mut self.ctx,
            fd,
            fs_rights_base,
            fs_rights_inheriting,
        )
        .map_err(anyhow_err_to_runtime_err)?;
        Ok(())
    }
}
