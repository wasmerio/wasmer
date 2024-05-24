use std::collections::{hash_map, BTreeMap};

use crate::os::task::process::MemorySnapshotRegion;

use super::*;

/// This value is tweaked to minimize the amount of journal
/// entries for a nominal workload but keep the resolution
/// high enough that it reduces overhead and inefficiency.
///
/// The test case used to tune this value was a HTTP server
/// serving a HTTP web page on hyper compiled to WASM. The
/// server was first warmed up with a bunch of requests then
/// the journal entries measured on subsequent requests, these
/// are the values
///
/// Resolution | Journal Size | Memory Overhead
/// -----------|--------------|----------------
/// 128 bytes  | 3584 bytes   | 12.5%
/// 256 bytes  | 4096 bytes   | 6.25%
/// 512 bytes  | 7680 bytes   | 3.12%
/// 1024 bytes | 12288 bytes  | 1.56%
/// 2048 bytes | 22528 bytes  | 0.78%
/// 4096 bytes | 32769 bytes  | 0.39%
///
/// Based on this data we have settled on 512 byte memory resolution
/// for region extents which keeps the journal size to a reasonable
/// value and the memory overhead of the hash table within an acceptable
/// limit
const MEMORY_REGION_RESOLUTION: u64 = 512;

impl JournalEffector {
    pub fn save_memory_and_snapshot(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        guard: &mut MutexGuard<'_, WasiProcessInner>,
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
        let mut regions = Vec::<MemorySnapshotRegion>::new();
        while cur < memory.data_size() {
            let mut again = false;
            let next = ((cur + MEMORY_REGION_RESOLUTION) / MEMORY_REGION_RESOLUTION)
                * MEMORY_REGION_RESOLUTION;
            let mut end = memory.data_size().min(next);
            for (_, thread) in guard.threads.iter() {
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

            let region = cur..end;
            regions.push(region.into());
            cur = end;
        }

        // Next we examine the dirty page manager and filter out any pages
        // that have not been explicitly written to (according to the
        // PTE)
        //
        // # TODO
        // https://docs.kernel.org/admin-guide/mm/soft-dirty.html

        // Now that we know all the regions that need to be saved we
        // enter a processing loop that dumps all the data to the log
        // file in an orderly manner.
        let memory = unsafe { env.memory_view(ctx) };
        let journal = ctx.data().active_journal()?;

        let mut regions_phase2 = BTreeMap::new();
        for region in regions.drain(..) {
            // We grab this region of memory as a vector and hash
            // it, which allows us to make some logging efficiency
            // gains.
            #[cfg(not(feature = "sys"))]
            let data = memory
                .copy_range_to_vec(region.into())
                .map_err(mem_error_to_wasi)?;

            // For x86 implementations running natively we have a
            // performance optimization that avoids a copy of the
            // memory when hashing for changed regions
            #[cfg(feature = "sys")]
            let data = {
                let d = unsafe { memory.data_unchecked() };
                if region.end > d.len() as u64 {
                    return Err(anyhow::anyhow!(
                        "memory access out of bounds ({} vs {})",
                        region.end,
                        d.len()
                    ));
                }
                &d[region.start as usize..region.end as usize]
            };

            // Compute a checksum and skip the memory if its already
            // been saved to the journal once already
            let hash = {
                let h: [u8; 32] = blake3::hash(data).into();
                u64::from_be_bytes([h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]])
            };
            match guard.snapshot_memory_hash.entry(region) {
                hash_map::Entry::Occupied(mut val) => {
                    if *val.get() == hash {
                        continue;
                    }
                    val.insert(hash);
                }
                hash_map::Entry::Vacant(vacant) => {
                    vacant.insert(hash);
                }
            }

            regions_phase2.insert(region, ());
        }

        // Combine regions together that are next to each other
        regions.clear();
        let mut last_end = None;
        for (region, _) in regions_phase2.iter() {
            if Some(region.start) == last_end {
                regions.last_mut().unwrap().end = region.end;
            } else {
                regions.push(*region);
            }
            last_end = Some(region.end);
        }

        // Perform the writes
        for region in regions {
            // We grab this region of memory as a vector and hash
            // it, which allows us to make some logging efficiency
            // gains.
            let data = memory
                .copy_range_to_vec(region.into())
                .map_err(mem_error_to_wasi)?;

            // Now we write it to the snap snapshot capturer
            journal
                .write(JournalEntry::UpdateMemoryRegionV1 {
                    region: region.into(),
                    data: data.into(),
                })
                .map_err(map_snapshot_err)?;
        }

        // Finally we mark the end of the snapshot so that
        // it can act as a restoration point
        let when = SystemTime::now();
        journal
            .write(JournalEntry::SnapshotV1 { when, trigger })
            .map_err(map_snapshot_err)?;

        // When writing snapshots we also flush the journal so that
        // its guaranteed to be on the disk or network pipe
        journal.flush().map_err(map_snapshot_err)?;
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
        mut data: &[u8],
    ) -> anyhow::Result<()> {
        let (env, mut store) = ctx.data_and_store_mut();

        let memory = unsafe { env.memory() };
        memory.grow_at_least(&mut store, region.end + data.len() as u64)?;

        // Write the data to the memory
        let memory = unsafe { env.memory_view(&store) };
        memory
            .write(region.start, data)
            .map_err(|err| WasiRuntimeError::Runtime(RuntimeError::user(err.into())))?;

        // Break the region down into chunks that align with the resolution
        let mut offset = region.start;
        while offset < region.end {
            let next = region.end.min(offset + MEMORY_REGION_RESOLUTION);
            let region = offset..next;
            offset = next;

            // Compute the hash and update it
            let size = region.end - region.start;
            let hash = {
                let h: [u8; 32] = blake3::hash(&data[..size as usize]).into();
                u64::from_be_bytes([h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]])
            };
            env.process
                .inner
                .0
                .lock()
                .unwrap()
                .snapshot_memory_hash
                .insert(region.into(), hash);

            // Shift the data pointer
            data = &data[size as usize..];
        }

        Ok(())
    }
}
