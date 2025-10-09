use super::*;

impl<'a> JournalSyscallPlayer<'a, '_> {
    #[allow(clippy::result_large_err)]
    pub(crate) unsafe fn action_update_compressed_memory(
        &mut self,
        region: Range<u64>,
        compressed_data: Cow<'a, [u8]>,
        differ_ethereal: Option<&mut Vec<JournalEntry<'a>>>,
    ) -> Result<(), WasiRuntimeError> {
        if Some(&self.cur_module_hash) != self.journal_module_hash.as_ref() {
            tracing::trace!("Ignored journal - UpdateMemory");
            return Ok(());
        }

        if self.bootstrapping {
            tracing::trace!("Differ(stage) journal - UpdateMemory");
            self.staged_differ_memory.push((region, compressed_data));
        } else if let Some(differ_ethereal) = differ_ethereal {
            tracing::trace!("Differ(ether) journal - UpdateMemory");
            differ_ethereal.push(JournalEntry::UpdateMemoryRegionV1 {
                region,
                compressed_data,
            });
        } else {
            tracing::trace!("Replay journal - UpdateMemory");
            unsafe {
                JournalEffector::apply_compressed_memory(&mut self.ctx, region, &compressed_data)
            }
            .map_err(anyhow_err_to_runtime_err)?;
        }
        Ok(())
    }
}
