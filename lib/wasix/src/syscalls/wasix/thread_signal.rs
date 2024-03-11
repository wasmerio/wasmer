use super::*;
use crate::syscalls::*;

/// ### `thread_signal()`
/// Send a signal to a particular thread in the current process.
/// Note: This is similar to `signal` in POSIX.
/// Inputs:
/// - `Signal`
///   Signal to be raised for this process
#[instrument(level = "debug", skip_all, fields(%tid, ?sig), ret)]
pub fn thread_signal(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    tid: Tid,
    sig: Signal,
) -> Result<Errno, WasiError> {
    {
        let tid: WasiThreadId = tid.into();
        ctx.data().process.signal_thread(&tid, sig);
    }

    let env = ctx.data();

    wasi_try_ok!(WasiEnv::process_signals_and_exit(&mut ctx)?);

    Ok(Errno::Success)
}
