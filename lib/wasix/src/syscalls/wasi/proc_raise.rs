use super::*;
use crate::syscalls::*;

/// ### `proc_raise()`
/// Send a signal to the process of the calling thread.
/// Note: This is similar to `raise` in POSIX.
/// Inputs:
/// - `Signal`
///   Signal to be raised for this process
#[instrument(level = "trace", skip_all, fields(sig), ret)]
pub fn proc_raise(mut ctx: FunctionEnvMut<'_, WasiEnv>, sig: Signal) -> Result<Errno, WasiError> {
    let env = ctx.data();
    env.process.signal_process(sig);

    WasiEnv::do_pending_operations(&mut ctx)?;

    Ok(Errno::Success)
}

/// ### `proc_raise_interval()`
/// Send a signal to the process of the calling thread with an interval.
/// Note: This is similar to `setitimer` in POSIX.
/// Inputs:
/// - `sig`: Signal to be raised for this process
/// - `interval`: The time interval in milliseconds
/// - `repeat`: Whether repeat the `sig` with `interval` or not
pub fn proc_raise_interval(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sig: Signal,
    interval: Timestamp,
    repeat: Bool,
) -> Result<Errno, WasiError> {
    let env = ctx.data();
    let interval = match interval {
        0 => None,
        a => Some(Duration::from_millis(a)),
    };
    let repeat = matches!(repeat, Bool::True);
    let _ = env
        .process
        .signal_interval(sig, interval, if repeat { interval } else { None });

    WasiEnv::do_pending_operations(&mut ctx)?;

    Ok(Errno::Success)
}
