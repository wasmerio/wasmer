use super::*;

impl JournalEffector {
    pub fn save_memory_and_snapshot(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        process: &mut MutexGuard<'_, WasiProcessInner>,
        trigger: SnapshotTrigger,
    ) -> anyhow::Result<()> {
        let env = ctx.data();
        let memory = unsafe { env.memory_view(ctx) };

        // Compute all the regions that we need to save which is basically
        // everything in the memory except for the memory stacks.
        //
        // We do not want the regions to be greater than 64KB as this will
        // otherwise create too much inefficiency. We choose 64KB as its
        // aligned with the standard WASM page size.
        let mut cur = 0u64;
        let mut regions = LinkedList::<Range<u64>>::new();
        while cur < memory.data_size() {
            let mut again = false;
            let mut end = memory.data_size().min(cur + 65536);
            for (_, thread) in process.threads.iter() {
                let layout = thread.memory_layout();
                if cur >= layout.stack_lower && cur < layout.stack_upper {
                    cur = layout.stack_upper;
                    again = true;
                    break;
                }
                if end > layout.stack_lower && end < layout.stack_upper {
                    end = end.min(layout.stack_lower);
                }
            }
            if again {
                continue;
            }
            regions.push_back(cur..end);
            cur = end;
        }

        // Now that we know all the regions that need to be saved we
        // enter a processing loop that dumps all the data to the log
        // file in an orderly manner.
        let memory = unsafe { env.memory_view(ctx) };
        let journal = ctx.data().active_journal()?;

        for region in regions {
            // We grab this region of memory as a vector and hash
            // it, which allows us to make some logging efficiency
            // gains.
            let data = memory
                .copy_range_to_vec(region.clone())
                .map_err(mem_error_to_wasi)?;

            // Now we write it to the snap snapshot capturer
            journal
                .write(JournalEntry::UpdateMemoryRegion {
                    region,
                    data: data.into(),
                })
                .map_err(map_snapshot_err)?;
        }

        // Finally we mark the end of the snapshot so that
        // it can act as a restoration point
        let when = SystemTime::now();
        journal
            .write(JournalEntry::Snapshot { when, trigger })
            .map_err(map_snapshot_err)?;
        Ok(())
    }

    /// # Safety
    ///
    /// This function manipulates the memory of the process and thus must be executed
    /// by the WASM process thread itself.
    ///
    pub unsafe fn apply_memory(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        region: Range<u64>,
        data: &[u8],
    ) -> anyhow::Result<()> {
        let (env, store) = ctx.data_and_store_mut();
        let memory = unsafe { env.memory_view(&store) };
        memory
            .write(region.start, data.as_ref())
            .map_err(|err| WasiRuntimeError::Runtime(RuntimeError::user(err.into())))?;
        Ok(())
    }
}
