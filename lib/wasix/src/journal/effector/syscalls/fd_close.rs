use super::*;
use crate::syscalls::shutdown_captured_handle;

impl JournalEffector {
    pub fn save_fd_close(ctx: &mut FunctionEnvMut<'_, WasiEnv>, fd: Fd) -> anyhow::Result<()> {
        Self::save_event(ctx, JournalEntry::CloseFileDescriptorV1 { fd })
    }

    pub fn apply_fd_close(ctx: &mut FunctionEnvMut<'_, WasiEnv>, fd: Fd) -> anyhow::Result<()> {
        let env = ctx.data();
        let (_, state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };
        let outcome = state.fs.close_fd_and_capture_shutdown(fd);

        if outcome.skipped_preopen {
            return Ok(());
        }

        if !outcome.removed {
            bail!(
                "journal restore error: failed to close descriptor (fd={fd}) - {}",
                Errno::Badf
            );
        }

        shutdown_captured_handle(env, outcome.shutdown_target).map_err(|err| {
            anyhow::anyhow!(
                "journal restore error: failed to drain before closing descriptor (fd={fd}) - {err:?}"
            )
        })?;

        Ok(())
    }
}
