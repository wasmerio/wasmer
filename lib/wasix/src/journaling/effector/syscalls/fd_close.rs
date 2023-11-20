use super::*;

impl JournalEffector {
    pub fn save_fd_close(ctx: &mut FunctionEnvMut<'_, WasiEnv>, fd: Fd) -> anyhow::Result<()> {
        Self::save_event(ctx, JournalEntry::CloseFileDescriptor { fd })
    }

    pub fn apply_fd_close(ctx: &mut FunctionEnvMut<'_, WasiEnv>, fd: Fd) -> anyhow::Result<()> {
        let env = ctx.data();
        let (_, state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };
        if let Err(err) = state.fs.close_fd(fd) {
            bail!(
                "snapshot restore error: failed to close descriptor (fd={}) - {}",
                fd,
                err
            );
        }
        Ok(())
    }
}
