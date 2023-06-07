use super::*;
use crate::syscalls::*;

/// ### `thread_id()`
/// Returns the index of the current thread
/// (threads indices are sequencial from zero)
#[instrument(level = "trace", skip_all, fields(tid = field::Empty), ret)]
pub fn thread_id<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    ret_tid: WasmPtr<Tid, M>,
) -> Errno {
    let env = ctx.data();
    let tid: Tid = env.thread.tid().into();
    Span::current().record("tid", tid);
    let memory = unsafe { env.memory_view(&ctx) };
    wasi_try_mem!(ret_tid.write(&memory, tid));
    Errno::Success
}
