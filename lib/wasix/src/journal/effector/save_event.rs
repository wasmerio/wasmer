use super::*;

impl JournalEffector {
    pub(crate) fn save_event(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        event: JournalEntry,
    ) -> anyhow::Result<()> {
        let env = ctx.data();
        if !env.should_journal() {
            tracing::trace!(
                "skipping journal event save (enable={}, replaying={})",
                env.enable_journal,
                env.replaying_journal
            );
            return Ok(());
        }

        tracing::trace!(?event, "saving journal event");

        ctx.data()
            .active_journal()?
            .write(event)
            .map_err(map_snapshot_err)?;
        Ok(())
    }
}
