use super::*;

impl JournalEffector {
    pub fn save_fd_pipe(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd1: Fd,
        fd2: Fd,
    ) -> anyhow::Result<()> {
        Self::save_event(ctx, JournalEntry::CreatePipeV1 { fd1, fd2 })
    }

    pub fn apply_fd_pipe(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd1: Fd,
        fd2: Fd,
    ) -> anyhow::Result<()> {
        let (ret_fd1, ret_fd2) = crate::syscalls::fd_pipe_internal(ctx).map_err(|err| {
            anyhow::format_err!("journal restore error: failed to create pipe - {}", err)
        })?;

        let ret = crate::syscalls::fd_renumber_internal(ctx, ret_fd1, fd1);
        if ret != Errno::Success {
            bail!(
                "journal restore error: failed renumber file descriptor after create pipe (from={}, to={}) - {}",
                ret_fd1,
                fd1,
                ret
            );
        }

        let ret = crate::syscalls::fd_renumber_internal(ctx, ret_fd2, fd2);
        if ret != Errno::Success {
            bail!(
                "journal restore error: failed renumber file descriptor after create pipe (from={}, to={}) - {}",
                ret_fd2,
                fd2,
                ret
            );
        }

        Ok(())
    }
}
