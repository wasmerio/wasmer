use super::*;
use crate::syscalls::*;

// NOTE: This syscall is not instrumented since it will be logged too much,
// hence introducing too much noise to the logs.

/// ### `clock_time_get()`
/// Get the time of the specified clock
///
/// Inputs:
///
/// - `Clockid clock_id`
///     The ID of the clock to query
/// - `Timestamp precision`
///     The maximum amount of error the reading may have
///
/// Output:
///
/// - `Timestamp *time`
///     The value of the clock in nanoseconds
#[cfg_attr(
    feature = "extra-logging",
    tracing::instrument(level = "trace", skip_all, ret)
)]
pub fn clock_time_get<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    clock_id: Snapshot0Clockid,
    precision: Timestamp,
    time: WasmPtr<Timestamp, M>,
) -> Result<Errno, WasiError> {
    ctx = wasi_try_ok!(maybe_backoff::<M>(ctx)?);

    let env = ctx.data();
    let memory = unsafe { env.memory_view(&ctx) };

    let mut t_out = wasi_try_ok!(platform_clock_time_get(clock_id, precision));
    {
        let guard = env.state.clock_offset.lock().unwrap();
        if let Some(offset) = guard.get(&clock_id) {
            t_out += *offset;
        }
    };
    wasi_try_mem_ok!(time.write(&memory, t_out as Timestamp));
    Ok(Errno::Success)
}
