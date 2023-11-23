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

        ctx.data()
            .active_journal()?
            .write(event)
            .map_err(map_snapshot_err)?;
        Ok(())
    }
}
