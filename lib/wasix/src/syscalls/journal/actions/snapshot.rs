use super::*;

impl<'a, 'c> JournalSyscallPlayer<'a, 'c> {
    #[allow(clippy::result_large_err)]
    pub(crate) unsafe fn action_snapshot(
        &mut self,
        when: SystemTime,
        trigger: SnapshotTrigger,
        differ_ethereal: Option<&mut Vec<JournalEntry<'a>>>,
    ) -> Result<(), WasiRuntimeError> {
        tracing::trace!("Replay journal - Snapshot");

        // If we are not in the same module then we fire off an exit
        // that simulates closing the process (hence keeps everything
        // in a clean state)
        let mut clear_ethereal = false;
        if self.journal_module_hash.is_some()
            && Some(self.cur_module_hash) != self.journal_module_hash
        {
            tracing::error!(
                "The WASM module hash does not match the journal module hash (journal_hash={:x?} vs module_hash{:x?}) - forcing a restart",
                self.journal_module_hash.unwrap(),
                self.cur_module_hash
            );
            self.clear_ethereal(differ_ethereal);
            return Ok(());
        }

        // Execute all the ethereal events
        if let Some(ethereal_events) = differ_ethereal {
            for next in ethereal_events.drain(..) {
                tracing::trace!("Ethereal snapshot event - {next:?}");
                self.play_event(next, None)?;
            }
            for (region, data) in self.staged_differ_memory.drain(..) {
                self.differ_memory.push((region, data));
            }
        }

        self.ctx.data_mut().pop_snapshot_trigger(trigger);
        Ok(())
    }
}
