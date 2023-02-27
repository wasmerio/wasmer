use super::*;
use crate::syscalls::*;

/// ### `thread_id()`
/// Returns the index of the current thread
/// (threads indices are sequencial from zero)
pub fn thread_id<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    ret_tid: WasmPtr<Tid, M>,
) -> Errno {
    /*
    trace!(
        "wasi[{}:{}]::thread_id",
        ctx.data().pid(),
        ctx.data().tid()
    );
    */

    let env = ctx.data();
    let tid: Tid = env.thread.tid().into();
    let memory = env.memory_view(&ctx);
    wasi_try_mem!(ret_tid.write(&memory, tid));
    Errno::Success
}
