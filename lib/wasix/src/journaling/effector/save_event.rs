use super::*;

impl JournalEffector {
    pub(crate) fn save_event(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        event: JournalEntry,
    ) -> anyhow::Result<()> {
        let env = ctx.data();
        if !env.should_journal() {
            return Ok(());
        }

        __asyncify_light(env, None, async {
            ctx.data()
                .active_journal()?
                .write(event)
                .await
                .map_err(map_snapshot_err)?;
            Ok(())
        })?
        .map_err(|err| WasiError::Exit(ExitCode::Errno(err)))?;

        Ok(())
    }
}
