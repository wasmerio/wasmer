use super::*;

impl JournalEffector {
    pub fn save_process_exit(env: &WasiEnv, exit_code: Option<ExitCode>) -> anyhow::Result<()> {
        env.active_journal()?
            .write(JournalEntry::ProcessExit { exit_code })
            .map_err(map_snapshot_err)?;
        Ok(())
    }

    pub fn apply_process_exit(env: &WasiEnv, exit_code: Option<ExitCode>) -> anyhow::Result<()> {
        env.blocking_on_exit(exit_code);
        Ok(())
    }
}
