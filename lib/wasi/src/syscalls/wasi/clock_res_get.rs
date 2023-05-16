use super::*;
use crate::syscalls::*;

/// ### `clock_res_get()`
/// Get the resolution of the specified clock
/// Input:
/// - `Clockid clock_id`
///     The ID of the clock to get the resolution of
/// Output:
/// - `Timestamp *resolution`
///     The resolution of the clock in nanoseconds
#[instrument(level = "trace", skip_all, ret)]
pub fn clock_res_get<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    clock_id: Snapshot0Clockid,
    resolution: WasmPtr<Timestamp, M>,
) -> Errno {
    let env = ctx.data();
    let memory = unsafe { env.memory_view(&ctx) };

    let out_addr = resolution.deref(&memory);
    let t_out = wasi_try!(platform_clock_res_get(clock_id, out_addr));
    wasi_try_mem!(resolution.write(&memory, t_out as Timestamp));
    Errno::Success
}
