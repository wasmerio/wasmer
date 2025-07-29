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
    let process = {
        let pid: WasiProcessId = pid.into();
        ctx.data().control_plane.get_process(pid)
    };
    if let Some(process) = process {
        process.signal_process(sig);
    }

    WasiEnv::do_pending_operations(&mut ctx)?;

    Ok(Errno::Success)
}
