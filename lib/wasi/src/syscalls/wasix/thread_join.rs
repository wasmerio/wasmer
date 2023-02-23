use super::*;
use crate::syscalls::*;

/// ### `thread_join()`
/// Joins this thread with another thread, blocking this
/// one until the other finishes
///
/// ## Parameters
///
/// * `tid` - Handle of the thread to wait on
pub fn thread_join(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    join_tid: Tid,
) -> Result<Errno, WasiError> {
    debug!(
        %join_tid,
        "wasi[{}:{}]::thread_join",
        ctx.data().pid(),
        ctx.data().tid(),
    );

    wasi_try_ok!(WasiEnv::process_signals_and_exit(&mut ctx)?);

    let env = ctx.data();
    let tid: WasiThreadId = join_tid.into();
    let other_thread = env.process.get_thread(&tid);
    if let Some(other_thread) = other_thread {
        wasi_try_ok!(__asyncify(&mut ctx, None, async move {
            other_thread.join().await;
            Ok(())
        })?);
        Ok(Errno::Success)
    } else {
        Ok(Errno::Success)
    }
}
