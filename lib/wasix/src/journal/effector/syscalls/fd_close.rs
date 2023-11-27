use super::*;

impl JournalEffector {
    pub fn save_fd_close(ctx: &mut FunctionEnvMut<'_, WasiEnv>, fd: Fd) -> anyhow::Result<()> {
        Self::save_event(ctx, JournalEntry::CloseFileDescriptorV1 { fd })
    }

    pub fn apply_fd_close(ctx: &mut FunctionEnvMut<'_, WasiEnv>, fd: Fd) -> anyhow::Result<()> {
        let env = ctx.data();
        let (_, state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };
        if let Err(err) = state.fs.close_fd(fd) {
            bail!(
                "journal restore error: failed to close descriptor (fd={}) - {}",
                fd,
                err
            );
        }
        Ok(())
    }
}
