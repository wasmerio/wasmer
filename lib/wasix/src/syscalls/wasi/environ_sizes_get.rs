use super::*;
use crate::{journal::SnapshotTrigger, syscalls::*};

/// ### `environ_sizes_get()`
/// Return command-line argument data sizes.
/// Outputs:
/// - `size_t *environ_count`
///     The number of environment variables.
/// - `size_t *environ_buf_size`
///     The size of the environment variable string data.
#[instrument(level = "trace", skip_all, ret)]
pub fn environ_sizes_get<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    environ_count: WasmPtr<M::Offset, M>,
    environ_buf_size: WasmPtr<M::Offset, M>,
) -> Result<Errno, WasiError> {
    ctx = wasi_try_ok!(maybe_snapshot_once::<M>(
        ctx,
        SnapshotTrigger::FirstEnviron
    )?);

    let env = ctx.data();
    let (memory, mut state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };

    let environ_count = environ_count.deref(&memory);
    let environ_buf_size = environ_buf_size.deref(&memory);

    let env_var_count: M::Offset = wasi_try_ok!(state
        .envs
        .lock()
        .unwrap()
        .len()
        .try_into()
        .map_err(|_| Errno::Overflow));
    let env_buf_size: usize = state.envs.lock().unwrap().iter().map(|v| v.len() + 1).sum();
    let env_buf_size: M::Offset =
        wasi_try_ok!(env_buf_size.try_into().map_err(|_| Errno::Overflow));
    wasi_try_mem_ok!(environ_count.write(env_var_count));
    wasi_try_mem_ok!(environ_buf_size.write(env_buf_size));

    trace!(
        %env_var_count,
        %env_buf_size
    );

    Ok(Errno::Success)
}
