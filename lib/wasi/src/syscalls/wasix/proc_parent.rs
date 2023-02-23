use super::*;
use crate::syscalls::*;

/// ### `proc_parent()`
/// Returns the parent handle of the supplied process
pub fn proc_parent<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    pid: Pid,
    ret_parent: WasmPtr<Pid, M>,
) -> Errno {
    debug!("wasi[{}:{}]::getppid", ctx.data().pid(), ctx.data().tid());

    let env = ctx.data();
    let pid: WasiProcessId = pid.into();
    if pid == env.process.pid() {
        let memory = env.memory_view(&ctx);
        wasi_try_mem!(ret_parent.write(&memory, env.process.ppid().raw() as Pid));
    } else {
        let control_plane = env.process.control_plane();
        if let Some(process) = control_plane.get_process(pid) {
            let memory = env.memory_view(&ctx);
            wasi_try_mem!(ret_parent.write(&memory, process.pid().raw() as Pid));
        } else {
            return Errno::Badf;
        }
    }
    Errno::Success
}
