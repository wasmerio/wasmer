use super::*;

/// Safety: This function manipulates the memory of the process and thus must
/// be executed by the WASM process thread itself.
///
#[allow(clippy::result_large_err)]
#[cfg(feature = "journal")]
pub unsafe fn restore_snapshot(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    journal: Arc<DynJournal>,
    bootstrapping: bool,
) -> Result<Option<RewindState>, WasiRuntimeError> {
    use std::{collections::BTreeMap, ops::Range};

    use crate::{journal::Journal, os::task::process::MemorySnapshotRegion};

    // Create the journal replay runner
    let mut runner = JournalSyscallPlayer::new(ctx, bootstrapping);

    // We read all the logs from the journal into the state machine
    let mut ethereal_events = Vec::new();
    while let Some(next) = journal.read().map_err(anyhow_err_to_runtime_err)? {
        runner.play_event(next.into_inner(), Some(&mut ethereal_events));
    }

    // Check for events that are orphaned
    for evt in ethereal_events {
        tracing::debug!("Orphaned ethereal events - {:?}", evt);
    }

    // Now output the stdout and stderr
    for (offset, data, is_64bit) in runner.stdout {
        if is_64bit {
            JournalEffector::apply_fd_write::<Memory64>(&runner.ctx, 1, offset, data)
        } else {
            JournalEffector::apply_fd_write::<Memory32>(&runner.ctx, 1, offset, data)
        }
        .map_err(anyhow_err_to_runtime_err)?;
    }

    for (offset, data, is_64bit) in runner.stderr {
        if is_64bit {
            JournalEffector::apply_fd_write::<Memory64>(&runner.ctx, 2, offset, data)
        } else {
            JournalEffector::apply_fd_write::<Memory32>(&runner.ctx, 2, offset, data)
        }
        .map_err(anyhow_err_to_runtime_err)?;
    }

    // Apply the memory changes (if this is in bootstrapping mode we differed them)
    for (region, data) in runner.differ_memory {
        tracing::trace!(
            "Replay journal - UpdateMemory - region:{:?}, data.len={}",
            region,
            data.len()
        );
        JournalEffector::apply_memory(&mut runner.ctx, region, &data)
            .map_err(anyhow_err_to_runtime_err)?;
    }

    // Spawn all the threads
    for (thread_id, thread_state) in runner.spawn_threads {
        if thread_state.is_64bit {
            JournalEffector::apply_thread_state::<Memory64>(
                &mut runner.ctx,
                thread_id,
                thread_state.memory_stack,
                thread_state.rewind_stack,
                thread_state.store_data,
                thread_state.start,
                thread_state.layout,
            )
            .map_err(anyhow_err_to_runtime_err)?;
        } else {
            JournalEffector::apply_thread_state::<Memory32>(
                &mut runner.ctx,
                thread_id,
                thread_state.memory_stack,
                thread_state.rewind_stack,
                thread_state.store_data,
                thread_state.start,
                thread_state.layout,
            )
            .map_err(anyhow_err_to_runtime_err)?;
        }
    }

    Ok(runner.rewind)
}
