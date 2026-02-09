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
    sig: i32,
) -> Result<Errno, WasiError> {
    let sig_u8 = match u8::try_from(sig) {
        Ok(value) => value,
        Err(_) => return Ok(Errno::Inval),
    };
    if sig_u8 > Signal::Sigsys as u8 {
        return Ok(Errno::Inval);
    }
    let sig = Signal::try_from(sig_u8).unwrap_or(Signal::Signone);

    let process = {
        let pid: WasiProcessId = pid.into();
        ctx.data().control_plane.get_process(pid)
    };
    let process = match process {
        Some(process) => process,
        None => return Ok(Errno::Srch),
    };

    if sig != Signal::Signone {
        process.signal_process(sig);
    }

    WasiEnv::do_pending_operations(&mut ctx)?;

    Ok(Errno::Success)
}
