use super::*;

impl<'a, 'c> JournalSyscallPlayer<'a, 'c> {
    #[allow(clippy::result_large_err)]
    pub(crate) unsafe fn action_fd_open(
        &mut self,
        fd: u32,
        dirfd: u32,
        dirflags: u32,
        path: Cow<'a, str>,
        o_flags: Oflags,
        fs_rights_base: Rights,
        fs_rights_inheriting: Rights,
        fs_flags: Fdflags,
    ) -> Result<(), WasiRuntimeError> {
        tracing::trace!(%fd, %dirfd, %dirflags,  "Replay journal - FdOpen {}", path);
        JournalEffector::apply_path_open(
            &mut self.ctx,
            fd,
            dirfd,
            dirflags,
            &path,
            o_flags,
            fs_rights_base,
            fs_rights_inheriting,
            fs_flags,
        )
        .map_err(anyhow_err_to_runtime_err)?;
        Ok(())
    }
}
