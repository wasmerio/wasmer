use super::*;
use crate::syscalls::*;

/// ### `sched_yield()`
/// Yields execution of the thread
#[instrument(level = "trace", skip_all, ret)]
pub fn sched_yield<M: MemorySize + 'static>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
) -> Result<Errno, WasiError> {
    ctx = wasi_try_ok!(maybe_backoff::<M>(ctx)?);

    //trace!("wasi[{}:{}]::sched_yield", ctx.data().pid(), ctx.data().tid());
    thread_sleep_internal::<M>(ctx, 0)
}
