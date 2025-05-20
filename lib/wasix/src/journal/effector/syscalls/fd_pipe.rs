use super::*;

impl JournalEffector {
    pub fn save_fd_pipe(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        read_fd: Fd,
        write_fd: Fd,
    ) -> anyhow::Result<()> {
        Self::save_event(ctx, JournalEntry::CreatePipeV1 { read_fd, write_fd })
    }

    pub fn apply_fd_pipe(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        read_fd: Fd,
        write_fd: Fd,
    ) -> anyhow::Result<()> {
        crate::syscalls::fd_pipe_internal(ctx, Some(read_fd), Some(write_fd)).map_err(|err| {
            anyhow::format_err!("journal restore error: failed to create pipe - {}", err)
        })?;

        Ok(())
    }
}
