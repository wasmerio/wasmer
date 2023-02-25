use super::*;
use crate::syscalls::*;

/// ### `environ_get()`
/// Read environment variable data.
/// The sizes of the buffers should match that returned by [`environ_sizes_get()`](#environ_sizes_get).
/// Inputs:
/// - `char **environ`
///     A pointer to a buffer to write the environment variable pointers.
/// - `char *environ_buf`
///     A pointer to a buffer to write the environment variable string data.
pub fn environ_get<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    environ: WasmPtr<WasmPtr<u8, M>, M>,
    environ_buf: WasmPtr<u8, M>,
) -> Errno {
    debug!(
        "wasi::environ_get. Environ: {:?}, environ_buf: {:?}",
        environ, environ_buf
    );
    let env = ctx.data();
    let (memory, mut state) = env.get_memory_and_wasi_state(&ctx, 0);
    trace!(" -> State envs: {:?}", state.envs);

    write_buffer_array(&memory, &*state.envs, environ, environ_buf)
}
