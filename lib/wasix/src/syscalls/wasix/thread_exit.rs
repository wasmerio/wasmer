use super::*;
use crate::syscalls::*;

/// ### `thread_exit()`
/// Terminates the current running thread, if this is the last thread then
/// the process will also exit with the specified exit code. An exit code
/// of 0 indicates successful termination of the thread. The meanings of
/// other values is dependent on the environment.
///
/// ## Parameters
///
/// * `rval` - The exit code returned by the process.
#[instrument(level = "debug", skip_all, fields(%exitcode), ret)]
pub fn thread_exit(ctx: FunctionEnvMut<'_, WasiEnv>, exitcode: ExitCode) -> Result<(), WasiError> {
    tracing::debug!(%exitcode);
    Err(WasiError::Exit(exitcode))
}
