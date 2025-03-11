use super::*;
use crate::syscalls::*;

/// Explicitly snapshots the process state.
#[instrument(level = "trace", skip_all, ret)]
pub fn proc_snapshot<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
) -> Result<Errno, WasiError> {
    // If we have an Explicit trigger, process that...
    ctx = wasi_try_ok!(maybe_snapshot_once::<M>(ctx, SnapshotTrigger::Explicit)?);
    // ... if not, we may still have an external request for a snapshot, so do that as well
    ctx = wasi_try_ok!(maybe_snapshot::<M>(ctx)?);
    Ok(Errno::Success)
}
