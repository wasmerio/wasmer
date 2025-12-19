use super::*;
use crate::{WasiVForkAsyncify, syscalls::*};

/// ### `proc_exit2()`
/// Terminate the process normally. A code of 0 indicates successful
/// termination of the program. The meanings of other values is dependent on
/// the environment.
/// Inputs:
/// - `ExitCode`
///   Exit code to return to the operating system
///
/// This is similar to `proc_exit` but it will switch back into the parent process
/// and return control there if we are in a `proc_fork_env` (vfork) context.
#[instrument(level = "trace", skip_all)]
pub fn proc_exit2<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    code: ExitCode,
) -> Result<(), WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    debug!(%code);

    // If we are in a vfork we need to return to the point we left off
    if let Some(mut vfork) = ctx.data_mut().vfork.take() {
        tracing::debug!(
            parent_pid = %vfork.env.process.pid(),
            child_pid = %ctx.data().process.pid(),
            "proc_exit from vfork, returning control to parent process"
        );

        // Prepare the child env for teardown by closing its FDs
        block_on(
            unsafe { ctx.data().get_memory_and_wasi_state(&ctx, 0) }
                .1
                .fs
                .close_all(),
        );

        // Restore the WasiEnv to the point when we vforked
        let mut parent_env = vfork.env;
        ctx.data_mut().swap_inner(parent_env.as_mut());
        let mut child_env = std::mem::replace(ctx.data_mut(), *parent_env);

        // Terminate the child process
        child_env.owned_handles.push(vfork.handle);
        child_env.process.terminate(code);

        let Some(asyncify_info) = vfork.asyncify else {
            // vfork without asyncify only forks the WasiEnv, which we have restored
            // above. We now return to the guest side in the parent process. Restoring
            // the control flow is done on the guest side.
            // See `proc_fork_env()` for information about this.

            return Ok(());
        };

        // Jump back to the vfork point and current on execution
        let child_pid = child_env.process.pid();
        let rewind_stack = asyncify_info.rewind_stack.freeze();
        let store_data = asyncify_info.store_data;
        unwind::<M, _>(ctx, move |mut ctx, _, _| {
            // Now rewind the previous stack and carry on from where we did the vfork
            match rewind::<M, _>(
                ctx,
                None,
                rewind_stack,
                store_data,
                ForkResult {
                    pid: child_pid.raw() as Pid,
                    ret: Errno::Success,
                },
            ) {
                Errno::Success => OnCalledAction::InvokeAgain,
                err => {
                    warn!("fork failed - could not rewind the stack - errno={}", err);
                    OnCalledAction::Trap(Box::new(WasiError::Exit(err.into())))
                }
            }
        })?;
        return Ok(());
    }

    // Otherwise just exit
    Err(WasiError::Exit(code))
}
