use super::*;

#[allow(clippy::extra_unused_type_parameters)]
#[cfg(not(feature = "journal"))]
pub fn maybe_snapshot<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
) -> WasiResult<FunctionEnvMut<'_, WasiEnv>> {
    Ok(Ok(ctx))
}

#[cfg(feature = "journal")]
pub fn maybe_snapshot<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
) -> WasiResult<FunctionEnvMut<'_, WasiEnv>> {
    use crate::os::task::process::{WasiProcessCheckpoint, WasiProcessInner};

    if !ctx.data().enable_journal {
        return Ok(Ok(ctx));
    }

    let inner = ctx.data().process.inner.clone();
    let res = wasi_try_ok_ok!(WasiProcessInner::maybe_checkpoint::<M>(inner, ctx)?);
    match res {
        MaybeCheckpointResult::Unwinding => return Ok(Err(Errno::Success)),
        MaybeCheckpointResult::NotThisTime(c) => {
            ctx = c;
        }
    }
    Ok(Ok(ctx))
}
