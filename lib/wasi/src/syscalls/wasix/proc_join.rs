use super::*;
use crate::syscalls::*;

/// ### `proc_join()`
/// Joins the child process, blocking this one until the other finishes
///
/// ## Parameters
///
/// * `pid` - Handle of the child process to wait on
pub fn proc_join<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    pid_ptr: WasmPtr<Pid, M>,
    exit_code_ptr: WasmPtr<ExitCode, M>,
) -> Result<Errno, WasiError> {
    wasi_try_ok!(WasiEnv::process_signals_and_exit(&mut ctx)?);

    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let pid = wasi_try_mem_ok!(pid_ptr.read(&memory));
    trace!(
        "wasi[{}:{}]::proc_join (pid={})",
        ctx.data().pid(),
        ctx.data().tid(),
        pid
    );

    // If the ID is maximum then it means wait for any of the children
    if pid == u32::MAX {
        let mut process = ctx.data_mut().process.clone();
        let child_exit = wasi_try_ok!(__asyncify(&mut ctx, None, async move {
            process.join_any_child().await
        })?);
        return match child_exit {
            Some((pid, exit_code)) => {
                trace!(
                    "wasi[{}:{}]::child ({}) exited with {}",
                    ctx.data().pid(),
                    ctx.data().tid(),
                    pid,
                    exit_code
                );
                let env = ctx.data();
                let memory = env.memory_view(&ctx);
                wasi_try_mem_ok!(pid_ptr.write(&memory, pid.raw() as Pid));
                wasi_try_mem_ok!(exit_code_ptr.write(&memory, exit_code));
                Ok(Errno::Success)
            }
            None => {
                trace!(
                    "wasi[{}:{}]::no children",
                    ctx.data().pid(),
                    ctx.data().tid()
                );
                let env = ctx.data();
                let memory = env.memory_view(&ctx);
                wasi_try_mem_ok!(pid_ptr.write(&memory, -1i32 as Pid));
                wasi_try_mem_ok!(exit_code_ptr.write(&memory, Errno::Child as u32));
                Ok(Errno::Child)
            }
        };
    }

    // Otherwise we wait for the specific PID
    let env = ctx.data();
    let pid: WasiProcessId = pid.into();
    let process = env.process.control_plane().get_process(pid);
    if let Some(process) = process {
        let exit_code = wasi_try_ok!(__asyncify(&mut ctx, None, async move {
            let code = process.join().await.unwrap_or(Errno::Child as u32);
            Ok(code)
        })?);

        trace!("child ({}) exited with {}", pid.raw(), exit_code);
        let env = ctx.data();
        let mut children = env.process.children.write().unwrap();
        children.retain(|a| *a != pid);

        let memory = env.memory_view(&ctx);
        wasi_try_mem_ok!(exit_code_ptr.write(&memory, exit_code));
        return Ok(Errno::Success);
    }

    debug!(
        "process already terminated or not registered (pid={})",
        pid.raw()
    );
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    wasi_try_mem_ok!(exit_code_ptr.write(&memory, Errno::Child as ExitCode));
    Ok(Errno::Child)
}
