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
///   The ID of the clock to query
/// - `Timestamp precision`
///   The maximum amount of error the reading may have
///
/// Output:
///
/// - `Timestamp *time`
///   The value of the clock in nanoseconds
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
    WasiEnv::do_pending_operations(&mut ctx)?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use wasmer::{Memory, MemoryType, Pages, Store};
    use wasmer_wasix_types::wasi::Snapshot0Clockid;

    // Helper to create a test memory
    fn create_test_memory() -> (Store, Memory) {
        let mut store = Store::default();
        let memory_type = MemoryType::new(Pages(1), None, false);
        let memory = Memory::new(&mut store, memory_type).unwrap();
        (store, memory)
    }

    #[test]
    fn test_clock_time_get_monotonic() {
        let result = platform_clock_time_get(Snapshot0Clockid::Monotonic, 0);
        assert!(result.is_ok(), "clock_time_get(MONOTONIC) should succeed");
        let time = result.unwrap();
        assert!(time > 0, "MONOTONIC time should be positive, got {}", time);
    }

    #[test]
    fn test_clock_time_get_realtime() {
        let result = platform_clock_time_get(Snapshot0Clockid::Realtime, 0);
        assert!(result.is_ok(), "clock_time_get(REALTIME) should succeed");
        let time = result.unwrap();
        assert!(time > 0, "REALTIME time should be positive");
    }

    #[test]
    fn test_clock_time_get_process_cputime() {
        let result = platform_clock_time_get(Snapshot0Clockid::ProcessCputimeId, 0);
        assert!(
            result.is_ok(),
            "clock_time_get(PROCESS_CPUTIME_ID) should succeed"
        );
        let time = result.unwrap();
        assert!(time >= 0, "PROCESS_CPUTIME time should be non-negative");
    }

    #[test]
    fn test_clock_time_get_thread_cputime() {
        let result = platform_clock_time_get(Snapshot0Clockid::ThreadCputimeId, 0);
        assert!(
            result.is_ok(),
            "clock_time_get(THREAD_CPUTIME_ID) should succeed"
        );
        let time = result.unwrap();
        assert!(time >= 0, "THREAD_CPUTIME time should be non-negative");
    }

    #[test]
    fn test_clock_time_precision_0_and_1() {
        // Test from wasmtime: Different precision values should work
        let result_p0 = platform_clock_time_get(Snapshot0Clockid::Monotonic, 0);
        assert!(result_p0.is_ok(), "precision 0 should work");

        let result_p1 = platform_clock_time_get(Snapshot0Clockid::Monotonic, 1);
        assert!(result_p1.is_ok(), "precision 1 should work");

        // Both should return valid times
        let time_p0 = result_p0.unwrap();
        let time_p1 = result_p1.unwrap();

        assert!(time_p0 > 0, "Time with precision 0 should be positive");
        assert!(time_p1 > 0, "Time with precision 1 should be positive");
    }

    #[test]
    fn test_clock_time_get_invalid_clock() {
        let result = platform_clock_time_get(Snapshot0Clockid::Unknown, 0);
        assert!(
            result.is_err(),
            "clock_time_get with Unknown clock should fail"
        );
        assert_eq!(result.unwrap_err(), Errno::Inval, "Should return EINVAL");
    }

    #[test]
    fn test_all_clocks_return_time() {
        let clocks = [
            Snapshot0Clockid::Realtime,
            Snapshot0Clockid::Monotonic,
            Snapshot0Clockid::ProcessCputimeId,
            Snapshot0Clockid::ThreadCputimeId,
        ];

        for clock_id in &clocks {
            let result = platform_clock_time_get(*clock_id, 0);

            assert!(
                result.is_ok(),
                "clock_time_get({:?}) should succeed",
                clock_id
            );
            let time = result.unwrap();
            assert!(
                time >= 0,
                "Time for {:?} should be non-negative, got {}",
                clock_id,
                time
            );
        }
    }

    #[test]
    fn test_clock_time_advances() {
        // Test from wasmtime: MONOTONIC clock should be monotonic (never decrease) and advance forward
        let time1 = platform_clock_time_get(Snapshot0Clockid::Monotonic, 0).unwrap();

        std::thread::sleep(std::time::Duration::from_millis(1));

        let time2 = platform_clock_time_get(Snapshot0Clockid::Monotonic, 0).unwrap();

        assert!(
            time2 > time1,
            "MONOTONIC clock should advance: time1={}, time2={}",
            time1,
            time2
        );
    }

    #[test]
    fn test_realtime_after_epoch() {
        // REALTIME should be after Unix epoch (Jan 1, 1970)
        // 1 billion seconds = ~September 2001
        // We're well past that now
        let time = platform_clock_time_get(Snapshot0Clockid::Realtime, 0).unwrap();

        let one_billion_seconds_ns = 1_000_000_000i64 * 1_000_000_000i64;
        assert!(
            time > one_billion_seconds_ns,
            "REALTIME should be after year 2001, got {}ns",
            time
        );

        // Also check it's not absurdly far in the future (before year 2100)
        // 4 billion seconds ≈ year 2096
        let four_billion_seconds_ns = 4_000_000_000i64 * 1_000_000_000i64;
        assert!(
            time < four_billion_seconds_ns,
            "REALTIME should be before year 2100, got {}ns",
            time
        );
    }

    #[test]
    fn test_stress_ng_repeated_calls() {
        // Stress test: 1000 repeated calls to clock_time_get
        let mut prev_time = 0i64;

        for i in 0..1000 {
            let result = platform_clock_time_get(Snapshot0Clockid::Monotonic, 0);
            assert!(
                result.is_ok(),
                "clock_time_get should succeed on iteration {}",
                i
            );

            let time = result.unwrap();
            assert!(time > 0, "Time should be positive on iteration {}", i);

            if i > 0 {
                assert!(
                    time >= prev_time,
                    "MONOTONIC should not decrease: iteration {} got {}, previous was {}",
                    i,
                    time,
                    prev_time
                );
            }

            prev_time = time;
        }
    }
}
