use super::*;

impl SnapshotEffector {
    pub(super) fn save_event(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        event: SnapshotLog,
    ) -> anyhow::Result<()> {
        let env = ctx.data();

        __asyncify_light(env, None, async {
            ctx.data()
                .runtime()
                .snapshot_capturer()
                .write(event)
                .await
                .map_err(map_snapshot_err)?;
            Ok(())
        })?
        .map_err(|err| WasiError::Exit(ExitCode::Errno(err)))?;

        Ok(())
    }
}
