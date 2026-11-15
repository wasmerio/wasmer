use super::*;
use crate::syscalls::*;

/// ### `proc_signal()`
/// Sends a signal to a child process
///
/// ## Parameters
///
/// * `pid` - Handle of the child process to wait on
/// * `sig` - Signal to send the child process
#[instrument(level = "trace", skip_all, fields(%pid, ?sig), ret)]
pub fn proc_signal(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    pid: Pid,
    sig: Signal,
) -> Result<Errno, WasiError> {
    // A non-positive PID addresses a process group or every process, as in
    // kill(2). WASIX has no process groups, and the caller's own group always
    // exists, so these stay a no-op rather than reporting ESRCH.
    let result = if (pid as i32) <= 0 {
        Errno::Success
    } else if let Some(process) = ctx.data().control_plane.get_process(pid.into()) {
        process.signal_process(sig);
        Errno::Success
    } else {
        Errno::Srch
    };

    WasiEnv::do_pending_operations(&mut ctx)?;

    Ok(result)
}
