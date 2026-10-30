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
    let process = if (pid as i32) <= 0 {
        None
    } else {
        let pid: WasiProcessId = pid.into();
        ctx.data().control_plane.get_process(pid)
    };
    let result = if (pid as i32) <= 0 {
        Errno::Success
    } else if let Some(process) = process {
        let threads = process.all_threads();
        process.signal_process(sig);
        if sig == Signal::Sigkill {
            for tid in threads {
                if let Err(error) = ctx.data().tasks().terminate_wasm_thread(process.pid(), tid) {
                    tracing::debug!(
                        %error,
                        pid = %process.pid(),
                        %tid,
                        "task manager could not terminate WASM thread",
                    );
                    process.wake_atomic_waiters(sig);
                }
            }
        }
        Errno::Success
    } else {
        Errno::Srch
    };

    WasiEnv::do_pending_operations(&mut ctx)?;

    Ok(result)
}
