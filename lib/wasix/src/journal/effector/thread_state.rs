use super::*;

impl JournalEffector {
    pub fn save_thread_state<M: MemorySize>(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        id: WasiThreadId,
        memory_stack: Bytes,
        rewind_stack: Bytes,
        store_data: Bytes,
    ) -> anyhow::Result<()> {
        Self::save_event(
            ctx,
            JournalEntry::SetThread {
                id,
                call_stack: Cow::Owned(rewind_stack.into()),
                memory_stack: Cow::Owned(memory_stack.into()),
                store_data: Cow::Owned(store_data.into()),
                is_64bit: M::is_64bit(),
            },
        )
    }
}
