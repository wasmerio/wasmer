use super::*;
use crate::syscalls::*;

/// ### `sched_yield()`
/// Yields execution of the thread
pub fn sched_yield(mut ctx: FunctionEnvMut<'_, WasiEnv>) -> Result<Errno, WasiError> {
    //trace!("wasi[{}:{}]::sched_yield", ctx.data().pid(), ctx.data().tid());
    let env = ctx.data();
    let tasks = env.tasks.clone();
    wasi_try_ok!(__asyncify(&mut ctx, None, async move {
        tasks.sleep_now(current_caller_id(), 0).await;
        Ok(())
    }));
    wasi_try_ok!(ctx.data().clone().process_signals_and_exit(&mut ctx)?);
    Ok(Errno::Success)
}
