use super::*;
use crate::syscalls::*;

/// ### `clock_time_set()`
/// Set the time of the specified clock
/// Inputs:
/// - `Clockid clock_id`
///     The ID of the clock to query
/// - `Timestamp *time`
///     The value of the clock in nanoseconds
pub fn clock_time_set<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    clock_id: Snapshot0Clockid,
    time: Timestamp,
) -> Errno {
    trace!(
        "wasi::clock_time_set clock_id: {:?}, time: {}",
        clock_id,
        time
    );
    let env = ctx.data();
    let memory = env.memory_view(&ctx);

    let precision = 1 as Timestamp;
    let t_now = wasi_try!(platform_clock_time_get(clock_id, precision));
    let t_now = t_now as i64;

    let t_target = time as i64;
    let t_offset = t_target - t_now;

    let mut guard = env.state.clock_offset.lock().unwrap();
    guard.insert(clock_id, t_offset);

    Errno::Success
}
