use super::*;

impl<'a, 'c> JournalSyscallPlayer<'a, 'c> {
    #[allow(clippy::result_large_err)]
    pub(crate) unsafe fn action_process_exit(
        &mut self,
        exit_code: Option<ExitCode>,
        differ_ethereal: Option<&mut Vec<JournalEntry<'a>>>,
    ) -> Result<(), WasiRuntimeError> {
        tracing::trace!(?exit_code, "Replay journal - ProcessExit");
        if self.bootstrapping {
            self.clear_ethereal(differ_ethereal);
            self.differ_memory.clear();
            self.rewind = None;
        } else {
            JournalEffector::apply_process_exit(&mut self.ctx, exit_code)
                .map_err(anyhow_err_to_runtime_err)?;
        }
        Ok(())
    }
}
