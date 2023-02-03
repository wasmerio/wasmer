use super::*;
use crate::syscalls::*;

/// ### `thread_signal()`
/// Send a signal to a particular thread in the current process.
/// Note: This is similar to `signal` in POSIX.
/// Inputs:
/// - `Signal`
///   Signal to be raised for this process
pub fn thread_signal(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    tid: Tid,
    sig: Signal,
) -> Result<Errno, WasiError> {
    debug!(
        "wasi[{}:{}]::thread_signal(tid={}, sig={:?})",
        ctx.data().pid(),
        ctx.data().tid(),
        tid,
        sig
    );
    {
        let tid: WasiThreadId = tid.into();
        ctx.data().process.signal_thread(&tid, sig);
    }

    let env = ctx.data();

    wasi_try_ok!(WasiEnv::process_signals_and_exit(&mut ctx)?);

    Ok(Errno::Success)
}
