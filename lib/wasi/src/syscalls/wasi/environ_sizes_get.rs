use super::*;
use crate::syscalls::*;

/// ### `environ_sizes_get()`
/// Return command-line argument data sizes.
/// Outputs:
/// - `size_t *environ_count`
///     The number of environment variables.
/// - `size_t *environ_buf_size`
///     The size of the environment variable string data.
pub fn environ_sizes_get<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    environ_count: WasmPtr<M::Offset, M>,
    environ_buf_size: WasmPtr<M::Offset, M>,
) -> Errno {
    trace!(
        "wasi[{}:{}]::environ_sizes_get",
        ctx.data().pid(),
        ctx.data().tid()
    );
    let env = ctx.data();
    let (memory, mut state) = env.get_memory_and_wasi_state(&ctx, 0);

    let environ_count = environ_count.deref(&memory);
    let environ_buf_size = environ_buf_size.deref(&memory);

    let env_var_count: M::Offset =
        wasi_try!(state.envs.len().try_into().map_err(|_| Errno::Overflow));
    let env_buf_size: usize = state.envs.iter().map(|v| v.len() + 1).sum();
    let env_buf_size: M::Offset = wasi_try!(env_buf_size.try_into().map_err(|_| Errno::Overflow));
    wasi_try_mem!(environ_count.write(env_var_count));
    wasi_try_mem!(environ_buf_size.write(env_buf_size));

    trace!(
        "env_var_count: {}, env_buf_size: {}",
        env_var_count,
        env_buf_size
    );

    Errno::Success
}
