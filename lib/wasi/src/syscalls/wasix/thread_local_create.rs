use super::*;
use crate::syscalls::*;

/// ### `thread_local_create()`
/// Create a thread local variable
/// If The web assembly process exports function named '_thread_local_destroy'
/// then it will be invoked when the thread goes out of scope and dies.
///
/// ## Parameters
///
/// * `user_data` - User data that will be passed to the destructor
///   when the thread variable goes out of scope
pub fn thread_local_create<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    user_data: TlUser,
    ret_key: WasmPtr<TlKey, M>,
) -> Errno {
    trace!(
        "wasi[{}:{}]::thread_local_create (user_data={})",
        ctx.data().pid(),
        ctx.data().tid(),
        user_data
    );
    let env = ctx.data();

    let key = {
        let mut inner = env.process.write();
        inner.thread_local_seed += 1;
        let key = inner.thread_local_seed;
        inner.thread_local_user_data.insert(key, user_data);
        key
    };

    let memory = env.memory_view(&ctx);
    wasi_try_mem!(ret_key.write(&memory, key));
    Errno::Success
}
