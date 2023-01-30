use super::*;
use crate::syscalls::*;

/// ### `clock_time_get()`
/// Get the time of the specified clock
/// Inputs:
/// - `Clockid clock_id`
///     The ID of the clock to query
/// - `Timestamp precision`
///     The maximum amount of error the reading may have
/// Output:
/// - `Timestamp *time`
///     The value of the clock in nanoseconds
pub fn clock_time_get<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    clock_id: Snapshot0Clockid,
    precision: Timestamp,
    time: WasmPtr<Timestamp, M>,
) -> Errno {
    /*
    debug!(
        "wasi::clock_time_get clock_id: {}, precision: {}",
        clock_id as u8, precision
    );
    */
    let env = ctx.data();
    let memory = env.memory_view(&ctx);

    let mut t_out = wasi_try!(platform_clock_time_get(clock_id, precision));
    {
        let guard = env.state.clock_offset.lock().unwrap();
        if let Some(offset) = guard.get(&clock_id) {
            t_out += *offset;
        }
    };
    wasi_try_mem!(time.write(&memory, t_out as Timestamp));

    /*
    trace!(
        "time: {} => {}",
        wasi_try_mem!(time.deref(&memory).read()),
        result
    );
    */
    Errno::Success
}
