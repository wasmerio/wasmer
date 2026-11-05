use super::*;
use crate::syscalls::*;

/// ### `thread_parallelism()`
/// Returns the available parallelism which is normally the
/// number of available cores that can run concurrently
#[instrument(level = "trace", skip_all, fields(parallelism = field::Empty), ret)]
pub fn thread_parallelism<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    ret_parallelism: WasmPtr<M::Offset, M>,
) -> Errno {
    let env = ctx.data();
    let parallelism = wasi_try!(env.tasks().thread_parallelism().map_err(|err| {
        let err: Errno = err.into();
        err
    }));
    Span::current().record("parallelism", parallelism);
    let parallelism: M::Offset = wasi_try!(parallelism.try_into().map_err(|_| Errno::Overflow));
    let memory = unsafe { env.memory_view(&ctx) };
    wasi_try_mem!(ret_parallelism.write(&memory, parallelism));
    Errno::Success
}
