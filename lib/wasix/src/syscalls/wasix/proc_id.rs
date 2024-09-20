use super::*;
use crate::syscalls::*;

/// ### `proc_id()`
/// Returns the handle of the current process
#[instrument(level = "trace", skip_all, fields(pid = field::Empty), ret)]
pub fn proc_id<M: MemorySize>(ctx: FunctionEnvMut<'_, WasiEnv>, ret_pid: WasmPtr<Pid, M>) -> Errno {
    let env = ctx.data();
    let memory = unsafe { env.memory_view(&ctx) };

    let pid = env.process.pid();
    Span::current().record("pid", pid.raw());

    wasi_try_mem!(ret_pid.write(&memory, pid.raw() as Pid));
    Errno::Success
}
