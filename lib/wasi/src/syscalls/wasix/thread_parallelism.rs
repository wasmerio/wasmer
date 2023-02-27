use super::*;
use crate::syscalls::*;

/// ### `thread_parallelism()`
/// Returns the available parallelism which is normally the
/// number of available cores that can run concurrently
pub fn thread_parallelism<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    ret_parallelism: WasmPtr<M::Offset, M>,
) -> Errno {
    debug!(
        "wasi[{}:{}]::thread_parallelism",
        ctx.data().pid(),
        ctx.data().tid()
    );

    let env = ctx.data();
    let parallelism = wasi_try!(env.tasks().thread_parallelism().map_err(|err| {
        let err: Errno = err.into();
        err
    }));
    let parallelism: M::Offset = wasi_try!(parallelism.try_into().map_err(|_| Errno::Overflow));
    let memory = env.memory_view(&ctx);
    wasi_try_mem!(ret_parallelism.write(&memory, parallelism));
    Errno::Success
}
