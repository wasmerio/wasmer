use super::*;

impl JournalEffector {
    pub fn save_thread_exit(
        env: &WasiEnv,
        id: WasiThreadId,
        exit_code: Option<ExitCode>,
    ) -> anyhow::Result<()> {
        env.active_journal()?
            .write(JournalEntry::CloseThread { id, exit_code })
            .map_err(map_snapshot_err)?;
        Ok(())
    }
}
