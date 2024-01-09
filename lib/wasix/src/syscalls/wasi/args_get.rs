use super::*;
use crate::syscalls::*;

/// ### `args_get()`
/// Read command-line argument data.
/// The sizes of the buffers should match that returned by [`args_sizes_get()`](#args_sizes_get).
/// Inputs:
/// - `char **argv`
///     A pointer to a buffer to write the argument pointers.
/// - `char *argv_buf`
///     A pointer to a buffer to write the argument string data.
///
#[instrument(level = "debug", skip_all, ret)]
pub fn args_get<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    argv: WasmPtr<WasmPtr<u8, M>, M>,
    argv_buf: WasmPtr<u8, M>,
) -> Errno {
    let env = ctx.data();
    let (memory, mut state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };

    let args = state
        .args
        .iter()
        .map(|a| a.as_bytes().to_vec())
        .collect::<Vec<_>>();
    let result = write_buffer_array(&memory, &args, argv, argv_buf);

    debug!(
        "args:\n{}",
        state
            .args
            .iter()
            .enumerate()
            .map(|(i, v)| format!("{:>20}: {}", i, v))
            .collect::<Vec<String>>()
            .join("\n")
    );

    result
}
