use super::*;
use crate::syscalls::*;

/// ### `proc_exit()`
/// Terminate the process normally. An exit code of 0 indicates successful
/// termination of the program. The meanings of other values is dependent on
/// the environment.
/// Inputs:
/// - `ExitCode`
///   Exit code to return to the operating system
#[instrument(level = "trace", skip_all)]
pub fn proc_exit<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    code: ExitCode,
) -> Result<(), WasiError> {
    let in_asyncify_based_vfork = ctx
        .data()
        .vfork
        .as_ref()
        .map(|v| v.asyncify.is_some())
        .unwrap_or(false);

    proc_exit2::<M>(ctx, code)?;

    // proc_exit2 returns in two cases:
    // 1. We are in a asyncify-based vfork, in which case on_called is set and magic will happen after returning
    // 2. We are in a setjmp/longjmp vfork, in which case we need to error out as returning from proc_exit is not allowed

    if in_asyncify_based_vfork {
        return Ok(());
    }

    tracing::error!(
        "Calling proc_exit in a vfork is undefined behaviour. Call _exit or _proc_exit2 instead."
    );
    Err(WasiError::Exit(ExitCode::from(129)))
}
