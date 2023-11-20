use super::*;

impl JournalEffector {
    pub fn save_process_exit(env: &WasiEnv, exit_code: Option<ExitCode>) -> anyhow::Result<()> {
        __asyncify_light(env, None, async {
            env.active_journal()?
                .write(JournalEntry::ProcessExit { exit_code })
                .await
                .map_err(map_snapshot_err)?;
            Ok(())
        })?
        .map_err(|err| WasiError::Exit(ExitCode::Errno(err)))?;
        Ok(())
    }

    pub fn apply_process_exit(env: &WasiEnv, exit_code: Option<ExitCode>) -> anyhow::Result<()> {
        env.blocking_on_exit(exit_code);
        Ok(())
    }
}
