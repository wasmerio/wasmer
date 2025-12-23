use super::*;
use crate::syscalls::*;

// NOTE: This syscall is not instrumented by default since it can be logged too frequently,
// particularly in Go runtimes, introducing excessive noise to the logs.

/// ### `sched_yield()`
/// Yields execution of the thread
#[cfg_attr(
    feature = "extra-logging",
    tracing::instrument(level = "trace", skip_all, ret)
)]
pub fn sched_yield<M: MemorySize + 'static>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    ctx = wasi_try_ok!(maybe_backoff::<M>(ctx)?);

    //trace!("wasi[{}:{}]::sched_yield", ctx.data().pid(), ctx.data().tid());
    thread_sleep_internal::<M>(ctx, 0)
}
