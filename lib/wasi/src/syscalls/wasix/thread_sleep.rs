use super::*;
use crate::syscalls::*;

/// ### `thread_sleep()`
/// Sends the current thread to sleep for a period of time
///
/// ## Parameters
///
/// * `duration` - Amount of time that the thread should sleep
pub fn thread_sleep(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    duration: Timestamp,
) -> Result<Errno, WasiError> {
    thread_sleep_internal(ctx, duration)
}

pub(crate) fn thread_sleep_internal(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    duration: Timestamp,
) -> Result<Errno, WasiError> {
    /*
    trace!(
        "wasi[{}:{}]::thread_sleep",
        ctx.data().pid(),
        ctx.data().tid()
    );
    */
    wasi_try_ok!(WasiEnv::process_signals_and_exit(&mut ctx)?);

    let env = ctx.data();

    #[cfg(feature = "sys-thread")]
    if duration == 0 {
        std::thread::yield_now();
    }

    if duration > 0 {
        let duration = Duration::from_nanos(duration as u64);
        let tasks = env.tasks().clone();
        wasi_try_ok!(__asyncify(&mut ctx, Some(duration), async move {
            // using an infinite async sleep here means we don't have to write the same event
            // handling loop code for signals and timeouts
            InfiniteSleep::default().await;
            unreachable!(
                "the timeout or signals will wake up this thread even though it waits forever"
            )
        })?);
    }
    Ok(Errno::Success)
}
