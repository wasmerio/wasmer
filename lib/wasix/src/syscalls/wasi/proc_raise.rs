use super::*;
use crate::syscalls::*;

/// ### `proc_raise()`
/// Send a signal to the process of the calling thread.
/// Note: This is similar to `raise` in POSIX.
/// Inputs:
/// - `Signal`
///   Signal to be raised for this process
#[instrument(level = "trace", skip_all, fields(sig), ret)]
pub fn proc_raise(mut ctx: FunctionEnvMut<'_, WasiEnv>, sig: Signal) -> Result<Errno, WasiError> {
    let env = ctx.data();
    env.process.signal_process(sig);

    WasiEnv::do_pending_operations(&mut ctx)?;

    Ok(Errno::Success)
}

/// ### `proc_raise()`
/// Send a signal to the process of the calling thread.
/// Note: This is similar to `raise` in POSIX.
/// Inputs:
/// - `Signal`
///   Signal to be raised for this process
pub fn proc_raise_interval(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sig: Signal,
    interval: Timestamp,
    repeat: Bool,
) -> Result<Errno, WasiError> {
    println!("called regular raise");
    let env = ctx.data();
    let interval = match interval {
        0 => None,
        a => Some(Duration::from_millis(a)),
    };
    let repeat = matches!(repeat, Bool::True);
    let _ = env
        .process
        .signal_interval(sig, interval, if repeat { interval } else { None });

    WasiEnv::do_pending_operations(&mut ctx)?;

    Ok(Errno::Success)
}

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

    let value = timeval_to_duration(new.value);
    let interval = timeval_to_duration(new.interval);

    println!("duration: {value:?} {interval:?}");

    let old_value = env.process.signal_interval(sig, value, interval);

    if !ret_old.is_null() {
        let ret_ptr = ret_old.deref(&memory);
        let zero = __wasi_timeval_t { sec: 0, usec: 0 };
        let ret_timer = match old_value {
            Some(old_value) => __wasi_itimerval_t {
                interval: if let Some(interval) = old_value.interval {
                    __wasi_timeval_t {
                        sec: interval.as_secs(),
                        usec: interval.subsec_micros().into(),
                    }
                } else {
                    zero
                },
                value: __wasi_timeval_t {
                    sec: old_value.current_value.as_secs(),
                    usec: old_value.current_value.subsec_micros().into(),
                },
            },
            None => __wasi_itimerval_t {
                interval: zero,
                value: zero,
            },
        };

        wasi_try_ok!(ret_ptr.write(ret_timer).map_err(crate::mem_error_to_wasi));
    }

    WasiEnv::do_pending_operations(&mut ctx)?;

    Ok(Errno::Success)
}

fn timeval_to_duration(tv: __wasi_timeval_t) -> Option<Duration> {
    if tv.sec == 0 && tv.usec == 0 {
        None
    } else {
        Some(Duration::from_secs(tv.sec) + Duration::from_micros(tv.usec))
    }
}
