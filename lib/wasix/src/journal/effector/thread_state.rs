use std::sync::Arc;

use wasmer_wasix_types::wasix::ThreadStartType;

use crate::{
    os::task::thread::{RewindResultType, WasiMemoryLayout},
    syscalls::thread_spawn_internal_using_layout,
    RewindState,
};

use super::*;

impl JournalEffector {
    pub fn save_thread_state<M: MemorySize>(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        id: WasiThreadId,
        memory_stack: Bytes,
        rewind_stack: Bytes,
        store_data: Bytes,
        start: ThreadStartType,
        layout: WasiMemoryLayout,
    ) -> anyhow::Result<()> {
        Self::save_event(
            ctx,
            JournalEntry::SetThreadV1 {
                id: id.raw(),
                call_stack: Cow::Owned(rewind_stack.into()),
                memory_stack: Cow::Owned(memory_stack.into()),
                store_data: Cow::Owned(store_data.into()),
                start,
                layout,
                is_64bit: M::is_64bit(),
            },
        )
    }

    /// This will take the supplied stacks and apply them to the memory region
    /// dedicated to this thread. After that it will spawn a WASM thread and
    // continue the thread where it left off, which may even mean it goes
    // straight back to sleep.
    pub fn apply_thread_state<M: MemorySize>(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        tid: WasiThreadId,
        memory_stack: Bytes,
        rewind_stack: Bytes,
        store_data: Bytes,
        start: ThreadStartType,
        layout: WasiMemoryLayout,
    ) -> anyhow::Result<()> {
        let start_ptr: M::Offset = match start {
            ThreadStartType::MainThread => {
                return Err(anyhow::format_err!(
                    "unable to restore a main thread via this method"
                ));
            }
            ThreadStartType::ThreadSpawn { start_ptr } => start_ptr
                .try_into()
                .map_err(|_| anyhow::format_err!("overflow while processing thread restoration"))?,
        };

        // Create the thread for this ID
        let thread_handle = Arc::new(ctx.data().process.new_thread_with_id(
            layout.clone(),
            start,
            tid,
        )?);

        // Now spawn the thread itself
        thread_spawn_internal_using_layout::<M>(
            ctx,
            thread_handle,
            layout.clone(),
            start_ptr,
            Some((
                RewindState {
                    memory_stack,
                    rewind_stack,
                    store_data,
                    start,
                    layout,
                    is_64bit: M::is_64bit(),
                },
                RewindResultType::RewindRestart,
            )),
        )
        .map_err(|err| anyhow::format_err!("failed to spawn thread - {}", err))?;

        Ok(())
    }
}
