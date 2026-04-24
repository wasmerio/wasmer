use super::*;
use crate::syscalls::*;

/// ### `proc_raise_interval2()`
/// Send a delayed signal to the process of the calling thread with an optional interval for repeated signaling.
/// Note: This is similar to `setitimer` in POSIX.
/// Inputs:
/// - `sig`: Signal to be raised for this process
/// - `new_ptr`: Pointer to the new value
/// - `ret_old`: Output pointer to the old value. If `null`, it's ignored.
pub fn proc_raise_interval2<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sig: Signal,
    new_ptr: WasmPtr<__wasi_itimerval_t, M>,
    ret_old: WasmPtr<__wasi_itimerval_t, M>,
) -> Result<Errno, WasiError> {
    let env = ctx.data();
    let memory = unsafe { env.memory_view(&ctx) };

    let new_ptr = new_ptr.deref(&memory);
    let new = wasi_try_ok!(new_ptr.read().map_err(crate::mem_error_to_wasi));

    let value = wasi_try_ok!(timeval_to_duration(new.value));
    let interval = wasi_try_ok!(timeval_to_duration(new.interval));

    let old_value = env.process.signal_interval(sig, value, interval);

    if !ret_old.is_null() {
        let ret_ptr = ret_old.deref(&memory);

        let ret_timer = match old_value {
            Some(old_value) => {
                let now = Duration::from_nanos(wasi_try_ok!(platform_clock_time_get(
                    Snapshot0Clockid::Monotonic,
                    1_000_000
                )) as u64);

                // IMPORTANT: Unlike Linux, the signal handlers run whenever there is a call to the host.
                // This means there is a chance that the timer is handled with some delay. When this is the
                // case, the remaining time before the timer ticks becomes negative. We treat it as `0` and
                // set the old timer's value to `0`.
                let old_remaining = old_value
                    .current_value
                    .checked_sub(
                        now.checked_sub(Duration::from_nanos(old_value.last_signal as u64))
                            .expect("current time is greater than a previously set time"),
                    )
                    .unwrap_or(Duration::ZERO);

                __wasi_itimerval_t {
                    interval: if let Some(interval) = old_value.interval {
                        interval.into()
                    } else {
                        __wasi_timeval_t::ZERO
                    },
                    value: old_remaining.into(),
                }
            }
            None => __wasi_itimerval_t {
                interval: __wasi_timeval_t::ZERO,
                value: __wasi_timeval_t::ZERO,
            },
        };

        wasi_try_ok!(ret_ptr.write(ret_timer).map_err(crate::mem_error_to_wasi));
    }

    WasiEnv::do_pending_operations(&mut ctx)?;

    Ok(Errno::Success)
}

fn timeval_to_duration(tv: __wasi_timeval_t) -> Result<Option<Duration>, Errno> {
    let dur: Duration = tv.try_into()?;

    if dur == Duration::ZERO {
        Ok(None)
    } else {
        Ok(Some(dur))
    }
}
