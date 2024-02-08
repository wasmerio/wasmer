use wasmer_wasix_types::wasix::ThreadStartType;

use crate::{os::task::thread::WasiMemoryLayout, syscalls::thread_spawn_internal_phase2};

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

    pub fn apply_thread_state<M: MemorySize>(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        id: WasiThreadId,
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
        let thread_handle = ctx.data().process.new_thread_with_id(layout, start, tid)?;

        // Now spawn the thread itself
        thread_spawn_internal_phase2(&mut ctx, thread_handle, layout, start_ptr)
            .map_err(|err| anyhow::format_err!("failed to spawn thread"))?;

        Ok(())
    }
}
