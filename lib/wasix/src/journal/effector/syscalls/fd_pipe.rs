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
        crate::syscalls::fd_pipe_internal(ctx, Some(fd1), Some(fd2)).map_err(|err| {
            anyhow::format_err!("journal restore error: failed to create pipe - {}", err)
        })?;

        Ok(())
    }
}
