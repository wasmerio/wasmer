use super::*;

impl SnapshotEffector {
    pub fn save_thread_exit(
        env: &WasiEnv,
        id: WasiThreadId,
        exit_code: Option<ExitCode>,
    ) -> anyhow::Result<()> {
        __asyncify_light(env, None, async {
            env.runtime()
                .snapshot_capturer()
                .write(SnapshotLog::CloseThread { id, exit_code })
                .await
                .map_err(map_snapshot_err)?;
            Ok(())
        })?
        .map_err(|err| WasiError::Exit(ExitCode::Errno(err)))?;
        Ok(())
    }
}
