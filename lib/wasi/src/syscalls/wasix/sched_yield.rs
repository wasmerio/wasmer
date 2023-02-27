use super::*;
use crate::syscalls::*;

/// ### `sched_yield()`
/// Yields execution of the thread
pub fn sched_yield(mut ctx: FunctionEnvMut<'_, WasiEnv>) -> Result<Errno, WasiError> {
    //trace!("wasi[{}:{}]::sched_yield", ctx.data().pid(), ctx.data().tid());
    thread_sleep_internal(ctx, 0)
}
