use super::*;
use crate::syscalls::*;

/// Explicitly snapshots the process state.
#[instrument(level = "trace", skip_all, ret)]
pub fn proc_snapshot<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
) -> Result<Errno, WasiError> {
    wasi_try_ok!(maybe_snapshot_once::<M>(ctx, SnapshotTrigger::Explicit)?);
    Ok(Errno::Success)
}
