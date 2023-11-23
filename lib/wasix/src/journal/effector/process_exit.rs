use virtual_mio::InlineWaker;

use super::*;

impl JournalEffector {
    pub fn save_process_exit(env: &WasiEnv, exit_code: Option<ExitCode>) -> anyhow::Result<()> {
        env.active_journal()?
            .write(JournalEntry::ProcessExit { exit_code })
            .map_err(map_snapshot_err)?;
        Ok(())
    }

    pub fn apply_process_exit(env: &WasiEnv, exit_code: Option<ExitCode>) -> anyhow::Result<()> {
        // If we are in the phase of replaying journals then we
        // close all the file descriptors but we don't actually send
        // any signals
        if env.replaying_journal {
            let state = env.state.clone();
            InlineWaker::block_on(state.fs.close_all());
        } else {
            env.blocking_on_exit(exit_code);
        }
        Ok(())
    }
}
