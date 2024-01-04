use virtual_mio::InlineWaker;

use super::*;

impl JournalEffector {
    pub fn save_process_exit(env: &WasiEnv, exit_code: Option<ExitCode>) -> anyhow::Result<()> {
        env.active_journal()?
            .write(JournalEntry::ProcessExitV1 { exit_code })
            .map_err(map_snapshot_err)?;
        Ok(())
    }

    /// # Safety
    ///
    /// This function manipulates the memory of the process and thus must be executed
    /// by the WASM process thread itself.
    ///
    pub unsafe fn apply_process_exit(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        exit_code: Option<ExitCode>,
    ) -> anyhow::Result<()> {
        let env = ctx.data();
        // If we are in the phase of replaying journals then we
        // close all the file descriptors but we don't actually send
        // any signals
        if env.replaying_journal {
            let state = env.state.clone();
            InlineWaker::block_on(state.fs.close_all());
        } else {
            env.blocking_on_exit(exit_code);
        }

        // Reset the memory back to a zero size
        let memory = ctx.data_mut().inner().memory().clone();
        memory.reset(ctx)?;
        Ok(())
    }
}
