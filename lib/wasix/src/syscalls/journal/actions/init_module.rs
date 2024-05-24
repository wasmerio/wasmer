use super::*;

impl<'a, 'c> JournalSyscallPlayer<'a, 'c> {
    #[allow(clippy::result_large_err)]
    pub(crate) unsafe fn action_init_module(
        &mut self,
        wasm_hash: Box<[u8]>,
        differ_ethereal: Option<&mut Vec<JournalEntry<'a>>>,
    ) -> Result<(), WasiRuntimeError> {
        tracing::trace!("Replay journal - InitModule {:?}", wasm_hash);
        self.clear_ethereal(differ_ethereal);
        self.differ_memory.clear();
        self.journal_module_hash.replace(wasm_hash);
        Ok(())
    }
}
