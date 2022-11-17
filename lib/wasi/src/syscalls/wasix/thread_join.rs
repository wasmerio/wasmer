use super::*;
use crate::syscalls::*;

/// ### `thread_join()`
/// Joins this thread with another thread, blocking this
/// one until the other finishes
///
/// ## Parameters
///
/// * `tid` - Handle of the thread to wait on
pub fn thread_join(mut ctx: FunctionEnvMut<'_, WasiEnv>, tid: Tid) -> Result<Errno, WasiError> {
    debug!("wasi::thread_join");
    debug!(
        "wasi[{}:{}]::thread_join(tid={})",
        ctx.data().pid(),
        ctx.data().tid(),
        tid
    );

    let env = ctx.data();
    let tid: WasiThreadId = tid.into();
    let other_thread = env.process.get_thread(&tid);
    if let Some(other_thread) = other_thread {
        wasi_try_ok!(__asyncify(&mut ctx, None, move |_| async move {
            other_thread.join().await;
            Ok(())
        }));
        Ok(Errno::Success)
    } else {
        Ok(Errno::Success)
    }
}
