use super::*;

impl<'a, 'c> JournalReplayRunner<'a, 'c> {
    pub(crate) unsafe fn action_update_memory(
        &mut self,
        region: Range<u64>,
        data: Cow<'a, [u8]>,
        differ_ethereal: Option<&mut Vec<JournalEntry<'a>>>,
    ) -> Result<(), WasiRuntimeError> {
        tracing::trace!("Replay journal - UpdateMemory");
        if Some(self.cur_module_hash) != self.journal_module_hash {
            return Ok(());
        }

        if self.bootstrapping {
            self.staged_differ_memory.push((region, data));
        } else if let Some(differ_ethereal) = differ_ethereal {
            differ_ethereal.push(JournalEntry::UpdateMemoryRegionV1 { region, data });
        } else {
            JournalEffector::apply_memory(&mut self.ctx, region, &data)
                .map_err(anyhow_err_to_runtime_err)?;
        }
        Ok(())
    }
}
