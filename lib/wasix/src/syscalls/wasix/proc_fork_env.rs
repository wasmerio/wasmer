use super::*;
use crate::{
    WasiThreadHandle, capture_store_snapshot,
    os::task::OwnedTaskStatus,
    runtime::task_manager::{TaskWasm, TaskWasmRunProperties},
    state::context_switching::ContextSwitchingEnvironment,
    syscalls::*,
};
use serde::{Deserialize, Serialize};
use wasmer::Memory;

/// ### `proc_fork2()`
/// Helper function for vforking.
/// Creates a new environment for a new subprocess.
/// This function only returns **once**. It does no weird things to the controlflow. After calling this function most syscalls will behave like if you were in a new process.
/// However it's undefined to call any other syscall than proc_exit and proc_exec. Although most of them should work fine. Also some other restrictions may apply.
///
/// The child pid will not be modified if this call fails
///
/// When calling proc_exit instead of terminating the current process, it will terminate the new process and switch back to the parent environment. proc_exit will return Errno::Success in that case
/// When calling proc_exec, the new process will be finalized and the new process will be created. Like with `proc_exit`, it will switch back to the parent environment. proc_exec will return Errno::Success in that case
#[instrument(level = "trace", skip_all, fields(pid = ctx.data().process.pid().raw()), ret)]
pub fn proc_fork_env<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    pid_ptr: WasmPtr<Pid, M>,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    if let Some(vfork) = ctx.data().vfork.as_ref() {
        warn!("nesting vforks is not supported");
        return Ok(Errno::Notsup);
    }

    // Fork the environment which will copy all the open file handlers
    // and associate a new context but otherwise shares things like the
    // file system interface. The handle to the forked process is stored
    // in the parent process context
    let (mut child_env, mut child_handle) = match ctx.data().fork() {
        Ok(p) => p,
        Err(err) => {
            debug!("could not fork process: {err}");
            // TODO: evaluate the appropriate error code, document it in the spec.
            return Ok(Errno::Perm);
        }
    };
    let env = ctx.data();

    // Add the child to the parent's list of children
    env.process.lock().children.push(child_env.process.clone());

    // Write the child's PID to the provided pointer
    let memory = unsafe { env.memory_view(&ctx) };
    wasi_try_mem_ok!(pid_ptr.write(&memory, child_env.pid().raw()));

    // Serialize the globals
    let serialized_globals: Bytes = capture_store_snapshot(&mut ctx.as_store_mut())
        .serialize()
        .unwrap()
        .into();

    // Swap the the current environment with the child environment
    child_env.swap_inner(ctx.data_mut());
    std::mem::swap(ctx.data_mut(), &mut child_env);

    let previous_vfork = ctx.data_mut().vfork.replace(WasiVFork {
        // The rewind stack is not required as we will not be rewinding with asyncify
        rewind_stack: None,
        store_data: serialized_globals,
        env: Box::new(child_env),
        handle: child_handle,
        is_64bit: M::is_64bit(),
    });
    assert!(previous_vfork.is_none()); // Already checked at the start of the function

    return Ok(Errno::Success);
}
