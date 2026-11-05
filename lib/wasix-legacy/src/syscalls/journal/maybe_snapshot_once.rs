use super::*;

#[allow(clippy::extra_unused_type_parameters)]
#[cfg(not(feature = "journal"))]
pub fn maybe_snapshot_once<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    _trigger: crate::journal::SnapshotTrigger,
) -> WasiResult<FunctionEnvMut<'_, WasiEnv>> {
    Ok(Ok(ctx))
}

#[cfg(feature = "journal")]
pub fn maybe_snapshot_once<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    trigger: crate::journal::SnapshotTrigger,
) -> WasiResult<FunctionEnvMut<'_, WasiEnv>> {
    use crate::os::task::process::{WasiProcessCheckpoint, WasiProcessInner};

    if unsafe { handle_rewind_ext_with_default::<M, ()>(&mut ctx, HandleRewindType::ResultLess) }
        .is_some()
    {
        return Ok(Ok(ctx));
    }

    if !ctx.data().enable_journal {
        return Ok(Ok(ctx));
    }

    if ctx.data_mut().pop_snapshot_trigger(trigger) {
        let inner = ctx.data().process.inner.clone();
        let res = wasi_try_ok_ok!(WasiProcessInner::checkpoint::<M>(
            inner,
            ctx,
            WasiProcessCheckpoint::Snapshot { trigger },
        )?);
        match res {
            MaybeCheckpointResult::Unwinding => return Ok(Err(Errno::Success)),
            MaybeCheckpointResult::NotThisTime(c) => {
                ctx = c;
            }
        }
    }
    Ok(Ok(ctx))
}
