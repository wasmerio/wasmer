use super::*;
use crate::{journal::SnapshotTrigger, syscalls::*};

/// ### `environ_get()`
/// Read environment variable data.
/// The sizes of the buffers should match that returned by [`environ_sizes_get()`](#environ_sizes_get).
/// Inputs:
/// - `char **environ`
///     A pointer to a buffer to write the environment variable pointers.
/// - `char *environ_buf`
///     A pointer to a buffer to write the environment variable string data.
#[instrument(level = "trace", skip_all, ret)]
pub fn environ_get<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    environ: WasmPtr<WasmPtr<u8, M>, M>,
    environ_buf: WasmPtr<u8, M>,
) -> Result<Errno, WasiError> {
    ctx = wasi_try_ok!(maybe_snapshot_once::<M>(
        ctx,
        SnapshotTrigger::FirstEnviron
    )?);

    let env = ctx.data();
    let (memory, mut state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };

    let envs = state.envs.lock().unwrap();
    Ok(write_buffer_array(&memory, &envs, environ, environ_buf))
}
