use super::*;
use crate::syscalls::*;

/// ### `thread_local_get()`
/// Gets the value of a thread local variable
///
/// ## Parameters
///
/// * `key` - Thread key that this local variable that was previous set
pub fn thread_local_get<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    key: TlKey,
    ret_val: WasmPtr<TlVal, M>,
) -> Errno {
    //trace!("wasi[{}:{}]::thread_local_get (key={})", ctx.data().pid(), ctx.data().tid(), key);
    let env = ctx.data();

    let val = {
        let current_thread = ctx.data().thread.tid();
        let guard = env.process.read();
        guard.thread_local.get(&(current_thread, key)).copied()
    };
    let val = val.unwrap_or_default();
    let memory = env.memory_view(&ctx);
    wasi_try_mem!(ret_val.write(&memory, val));
    Errno::Success
}
