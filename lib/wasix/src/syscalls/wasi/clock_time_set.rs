use super::*;
use crate::syscalls::*;

/// ### `clock_time_set()`
/// Set the time of the specified clock
/// Inputs:
/// - `Clockid clock_id`
///     The ID of the clock to query
/// - `Timestamp *time`
///     The value of the clock in nanoseconds
#[instrument(level = "trace", skip_all, fields(?clock_id, %time), ret)]
pub fn clock_time_set<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    clock_id: Snapshot0Clockid,
    time: Timestamp,
) -> Result<Errno, WasiError> {
    let ret = clock_time_set_internal(&mut ctx, clock_id, time);
    let env = ctx.data();

    if ret == Errno::Success {
        #[cfg(feature = "journal")]
        if env.enable_journal {
            JournalEffector::save_clock_time_set(&mut ctx, clock_id, time).map_err(|err| {
                tracing::error!("failed to save clock time set event - {}", err);
                WasiError::Exit(ExitCode::Errno(Errno::Fault))
            })?;
        }
    }
    Ok(ret)
}

pub fn clock_time_set_internal(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    clock_id: Snapshot0Clockid,
    time: Timestamp,
) -> Errno {
    let env = ctx.data();
    let memory = unsafe { env.memory_view(&ctx) };

    let precision = 1 as Timestamp;
    let t_now = wasi_try!(platform_clock_time_get(clock_id, precision));

    let t_target = time as i64;
    let t_offset = t_target - t_now;

    let mut guard = env.state.clock_offset.lock().unwrap();
    guard.insert(clock_id, t_offset);
    Errno::Success
}
