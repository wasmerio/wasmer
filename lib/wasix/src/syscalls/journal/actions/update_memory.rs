use super::*;

impl<'a, 'c> JournalSyscallPlayer<'a, 'c> {
    #[allow(clippy::result_large_err)]
    pub(crate) unsafe fn action_update_memory(
        &mut self,
        region: Range<u64>,
        data: Cow<'a, [u8]>,
        differ_ethereal: Option<&mut Vec<JournalEntry<'a>>>,
    ) -> Result<(), WasiRuntimeError> {
        if Some(self.cur_module_hash) != self.journal_module_hash {
            tracing::trace!("Ignored journal - UpdateMemory");
            return Ok(());
        }

        if self.bootstrapping {
            tracing::trace!("Differ(stage) journal - UpdateMemory");
            self.staged_differ_memory.push((region, data));
        } else if let Some(differ_ethereal) = differ_ethereal {
            tracing::trace!("Differ(ether) journal - UpdateMemory");
            differ_ethereal.push(JournalEntry::UpdateMemoryRegionV1 { region, data });
        } else {
            tracing::trace!("Replay journal - UpdateMemory");
            JournalEffector::apply_memory(&mut self.ctx, region, &data)
                .map_err(anyhow_err_to_runtime_err)?;
        }
        Ok(())
    }
}
