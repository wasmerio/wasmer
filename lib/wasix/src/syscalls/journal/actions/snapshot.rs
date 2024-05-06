use super::*;

impl<'a, 'c> JournalSyscallPlayer<'a, 'c> {
    #[allow(clippy::result_large_err)]
    pub(crate) unsafe fn action_snapshot(
        &mut self,
        when: SystemTime,
        trigger: SnapshotTrigger,
        differ_ethereal: Option<&mut Vec<JournalEntry<'a>>>,
    ) -> Result<(), WasiRuntimeError> {
        // If we are not in the same module then we fire off an exit
        // that simulates closing the process (hence keeps everything
        // in a clean state)
        let mut clear_ethereal = false;
        if self.journal_module_hash.is_some()
            && Some(&self.cur_module_hash) != self.journal_module_hash.as_ref()
        {
            tracing::error!(
                "The WASM module hash does not match the journal module hash (journal_hash={:x?} vs module_hash{:x?}) - forcing a restart",
                self.journal_module_hash.as_ref().unwrap(),
                self.cur_module_hash
            );
            self.clear_ethereal(differ_ethereal);
            return Ok(());
        }

        tracing::trace!("Replay journal - Snapshot (trigger={:?})", trigger);

        // Execute all the ethereal events
        if let Some(ethereal_events) = differ_ethereal {
            for next in ethereal_events.drain(..) {
                tracing::trace!("Replay(ether) snapshot event - {next:?}");
                if let Err(err) = self.play_event(next, None) {
                    tracing::warn!("failed to replay event - {}", err);
                    return Err(err);
                }
            }
            for (region, data) in self.staged_differ_memory.drain(..) {
                tracing::trace!(
                    "Differ(end) memory event - {region:?} data.len={}",
                    data.len()
                );
                self.differ_memory.push((region, data));
            }
        }

        self.ctx.data_mut().pop_snapshot_trigger(trigger);
        Ok(())
    }
}
