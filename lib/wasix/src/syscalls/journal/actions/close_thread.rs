use super::*;

impl<'a, 'c> JournalSyscallPlayer<'a, 'c> {
    #[allow(clippy::result_large_err)]
    pub(crate) unsafe fn action_close_thread(
        &mut self,
        id: u32,
        exit_code: Option<ExitCode>,
        differ_ethereal: Option<&mut Vec<JournalEntry<'a>>>,
    ) -> Result<(), WasiRuntimeError> {
        if id == self.ctx.data().tid().raw() {
            tracing::trace!(%id, ?exit_code, "Replay journal - CloseThread(main)");
            if self.bootstrapping {
                self.clear_ethereal(differ_ethereal);
                self.staged_differ_memory.clear();
                self.differ_memory.clear();
                self.rewind = None;
            } else {
                JournalEffector::apply_process_exit(&mut self.ctx, exit_code)
                    .map_err(anyhow_err_to_runtime_err)?;
            }
        } else if let Some(differ_ethereal) = differ_ethereal {
            tracing::trace!(%id, ?exit_code, "Differ(end) journal - CloseThread");
            differ_ethereal.push(JournalEntry::CloseThreadV1 { id, exit_code });
        } else {
            tracing::trace!(%id, ?exit_code, "Replay journal - CloseThread");
            JournalEffector::apply_thread_exit(
                &mut self.ctx,
                Into::<WasiThreadId>::into(id),
                exit_code,
            )
            .map_err(anyhow_err_to_runtime_err)?;
        }
        Ok(())
    }
}
