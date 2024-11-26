use super::*;
use crate::syscalls::*;

/// ### `thread_exit()`
/// Terminates the current running thread, if this is the last thread then
/// the process will also exit with code 0.
/// The exit code parameter is a left over from a previous version of this
/// syscall, maintained here to keep the syscall backwards-compatible, but
/// is otherwise unused.
///
/// This syscall does not return.
#[instrument(level = "trace", skip_all, fields(%_exitcode), ret)]
pub fn thread_exit(ctx: FunctionEnvMut<'_, WasiEnv>, _exitcode: ExitCode) -> Result<(), WasiError> {
    tracing::debug!(tid=%ctx.data().thread.id(), "thread exit");
    Err(WasiError::ThreadExit)
}
