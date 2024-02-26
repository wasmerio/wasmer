use super::*;

impl<'a, 'c> JournalSyscallPlayer<'a, 'c> {
    #[allow(clippy::result_large_err)]
    pub(crate) unsafe fn action_set_thread(
        &mut self,
        id: u32,
        call_stack: Cow<'a, [u8]>,
        memory_stack: Cow<'a, [u8]>,
        store_data: Cow<'a, [u8]>,
        is_64bit: bool,
        start: ThreadStartType,
        layout: WasiMemoryLayout,
        differ_ethereal: Option<&mut Vec<JournalEntry<'a>>>,
    ) -> Result<(), WasiRuntimeError> {
        if Some(self.cur_module_hash) != self.journal_module_hash {
            tracing::trace!(%id, "Skipping journal entry - SetThread call_stack={} bytes memory_stack={} bytes store_data={} bytes", call_stack.len(), memory_stack.len(), store_data.len());
            return Ok(());
        }

        let state = RewindState {
            memory_stack: memory_stack.to_vec().into(),
            rewind_stack: call_stack.to_vec().into(),
            store_data: store_data.to_vec().into(),
            start,
            layout: layout.clone(),
            is_64bit,
        };

        if Into::<WasiThreadId>::into(id) == self.ctx.data().tid() {
            tracing::trace!(%id, "Differ(end) journal - SetThread(main) call_stack={} bytes memory_stack={} bytes store_data={} bytes", call_stack.len(), memory_stack.len(), store_data.len());
            self.rewind.replace(state);
        } else if let Some(differ_ethereal) = differ_ethereal {
            tracing::trace!(%id, "Differ(ether) journal - SetThread call_stack={} bytes memory_stack={} bytes store_data={} bytes", call_stack.len(), memory_stack.len(), store_data.len());
            differ_ethereal.push(JournalEntry::SetThreadV1 {
                id,
                call_stack,
                memory_stack,
                store_data,
                start,
                layout,
                is_64bit,
            });
        } else if self.bootstrapping {
            tracing::trace!(%id, "Differ(end) journal - SetThread({id}) call_stack={} bytes memory_stack={} bytes store_data={} bytes", call_stack.len(), memory_stack.len(), store_data.len());
            self.spawn_threads.insert(id.into(), state);
        } else {
            return Err(WasiRuntimeError::Runtime(RuntimeError::user(
                anyhow::format_err!(
                    "Snapshot restoration does not currently support live updates of running threads."
                )
                .into(),
            )));
        }
        Ok(())
    }
}
