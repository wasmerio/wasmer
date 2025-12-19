use crate::{WasiEnv, WasiError, WasiVFork};
use wasmer::{FunctionEnvMut, Memory, MemorySize, WasmPtr};
use wasmer_wasix_types::wasi::{Errno, Pid};

/// ### `proc_fork_env()`
/// Forks the environment of the current process into a new child process.
/// The child process will start with the same memory and execution context
/// as the parent process, similar to `fork()`.
///
/// Most syscalls are undefined behavior in the child process, except for
/// `proc_exit2()` and `proc_exec3()`. `proc_exit2()` will terminate the
/// child process and set the environment back to the parent process.
/// `proc_exec3()` will exec the module in the child process and promote
/// the child process to a real process. Then it will return with the
/// environment back to the parent process.
///
/// This function differs from a traditional `vfork` in that it does not
/// modify the control flow of the program. Instead, it only forks the
/// WasiEnv, but leaves everything else (memory, call stack, store etc.)
/// untouched.
///
/// This function is intended to be used in conjunction with
/// setjmp/longjmp to build a lightweight implementation of vforking.
///
/// The value at child_pid_ptr will only be modified by a successful
/// call to this function. It will contain the process id of the
/// child process.
#[tracing::instrument(level = "trace", skip_all, fields(pid = ctx.data().process.pid().raw()), ret)]
pub fn proc_fork_env<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    child_pid_ptr: WasmPtr<Pid, M>,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let env = ctx.data();

    if let Some(vfork) = env.vfork.as_ref() {
        tracing::warn!("nesting vforks is not supported");
        return Ok(Errno::Notsup);
    }

    // Fork the environment which will copy all the open file handlers
    // and associate a new context but otherwise shares things like the
    // file system interface. The handle to the forked process is stored
    // in the parent process context
    let (mut child_env, mut child_handle) = match env.fork() {
        Ok(p) => p,
        Err(err) => {
            tracing::error!("could not fork process: {err}");
            // TODO: evaluate the appropriate error code, document it in the spec.
            return Ok(Errno::Perm);
        }
    };

    // Write the child's PID to the provided pointer
    let memory = unsafe { env.memory_view(&ctx) };
    wasi_try_mem_ok!(child_pid_ptr.write(&memory, child_env.pid().raw()));

    let parent_env = ctx.data_mut();

    // Add the child to the parent's list of children
    parent_env
        .process
        .lock()
        .children
        .push(child_env.process.clone());
    // Swap the current environment with the child environment
    child_env.swap_inner(parent_env);
    std::mem::swap(parent_env, &mut child_env);

    let previous_vfork = parent_env.vfork.replace(WasiVFork {
        asyncify: None,
        env: Box::new(child_env),
        handle: child_handle,
    });
    assert!(previous_vfork.is_none()); // Already checked at the start of the function

    return Ok(Errno::Success);
}
