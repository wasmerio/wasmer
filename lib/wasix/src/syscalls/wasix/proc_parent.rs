use super::*;
use crate::syscalls::*;

/// ### `proc_parent()`
/// Returns the parent handle of the supplied process
#[instrument(level = "debug", skip_all, fields(%pid, parent = field::Empty), ret)]
pub fn proc_parent<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    pid: Pid,
    ret_parent: WasmPtr<Pid, M>,
) -> Errno {
    let env = ctx.data();
    let pid: WasiProcessId = pid.into();
    if pid == env.process.pid() {
        let memory = unsafe { env.memory_view(&ctx) };
        Span::current().record("parent", env.process.ppid().raw());
        wasi_try_mem!(ret_parent.write(&memory, env.process.ppid().raw() as Pid));
        Errno::Success
    } else if let Some(process) = env.control_plane.get_process(pid) {
        let memory = unsafe { env.memory_view(&ctx) };
        Span::current().record("parent", process.pid().raw());
        wasi_try_mem!(ret_parent.write(&memory, process.pid().raw() as Pid));
        Errno::Success
    } else {
        Errno::Badf
    }
}
