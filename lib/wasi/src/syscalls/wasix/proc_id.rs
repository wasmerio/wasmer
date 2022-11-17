use super::*;
use crate::syscalls::*;

/// ### `proc_id()`
/// Returns the handle of the current process
pub fn proc_id<M: MemorySize>(ctx: FunctionEnvMut<'_, WasiEnv>, ret_pid: WasmPtr<Pid, M>) -> Errno {
    debug!("wasi[{}:{}]::getpid", ctx.data().pid(), ctx.data().tid());

    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let pid = env.process.pid();
    wasi_try_mem!(ret_pid.write(&memory, pid.raw() as Pid));
    Errno::Success
}
