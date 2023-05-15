use super::*;
use crate::syscalls::*;

/// ### `proc_exit()`
/// Terminate the process normally. An exit code of 0 indicates successful
/// termination of the program. The meanings of other values is dependent on
/// the environment.
/// Inputs:
/// - `ExitCode`
///   Exit code to return to the operating system
#[instrument(level = "debug", skip_all)]
pub fn proc_exit<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    code: ExitCode,
) -> Result<(), WasiError> {
    debug!(%code);

    // If we are in a vfork we need to return to the point we left off
    if let Some(mut vfork) = ctx.data_mut().vfork.take() {
        // Restore the WasiEnv to the point when we vforked
        vfork.env.swap_inner(ctx.data_mut());
        std::mem::swap(vfork.env.as_mut(), ctx.data_mut());
        let mut wasi_env = *vfork.env;
        wasi_env.owned_handles.push(vfork.handle);

        // The child environment needs to be notified as exited
        wasi_env.thread.set_status_finished(Ok(code));

        // We still need to create the process that exited so that
        // the exit code can be used by the parent process
        let pid = wasi_env.process.pid();
        let mut memory_stack = vfork.memory_stack;
        let rewind_stack = vfork.rewind_stack;
        let store_data = vfork.store_data;

        // If the return value offset is within the memory stack then we need
        // to update it here rather than in the real memory
        let val_bytes = pid.raw().to_ne_bytes();
        let pid_offset: u64 = vfork.pid_offset;
        if pid_offset >= wasi_env.layout.stack_lower
            && (pid_offset + val_bytes.len() as u64) <= wasi_env.layout.stack_upper
        {
            // Make sure its within the "active" part of the memory stack
            let offset = wasi_env.layout().stack_upper - pid_offset;
            if (offset as usize + val_bytes.len()) > memory_stack.len() {
                warn!(
                    "fork failed - the return value (pid) is outside of the active part of the memory stack ({} vs {})",
                    offset,
                    memory_stack.len()
                );
                return Err(WasiError::Exit(Errno::Memviolation.into()));
            }

            // Update the memory stack with the new PID
            let pstart = memory_stack.len() - offset as usize;
            let pend = pstart + val_bytes.len();
            let pbytes = &mut memory_stack[pstart..pend];
            pbytes.clone_from_slice(&val_bytes);
        } else {
            warn!(
                "fork failed - the return value (pid) is not being returned on the stack - which is not supported"
            );
            return Err(WasiError::Exit(Errno::Memviolation.into()));
        }

        // Jump back to the vfork point and current on execution
        unwind::<M, _>(ctx, move |mut ctx, _, _| {
            // Now rewind the previous stack and carry on from where we did the vfork
            match rewind::<M>(
                ctx,
                memory_stack.freeze(),
                rewind_stack.freeze(),
                store_data,
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
