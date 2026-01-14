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
    fn test_clock_res_realtime() {
        let (mut store, memory) = create_test_memory();
        let view = memory.view(&store);

        // Initialize with large value to ensure it gets overwritten
        let resolution_ptr: WasmPtr<u64> = WasmPtr::new(0);
        resolution_ptr.write(&view, 100000).unwrap();

        let resolution_ref = resolution_ptr.deref(&view);
        let result = platform_clock_res_get(Snapshot0Clockid::Realtime, resolution_ref);

        assert!(result.is_ok(), "clock_res_get(REALTIME) should succeed");

        // Write the returned resolution to memory
        let resolution_value = result.unwrap() as u64;
        resolution_ptr.write(&view, resolution_value).unwrap();

        let resolution = resolution_ptr.read(&view).unwrap();
        assert_ne!(
            resolution, 100000,
            "Resolution should be overwritten from initial value"
        );
        assert!(resolution > 0, "Resolution should be positive");
        assert!(
            resolution <= 1_000_000_000,
            "Resolution should be <= 1 second"
        );
    }

    #[test]
    fn test_clock_res_monotonic() {
        let (mut store, memory) = create_test_memory();
        let view = memory.view(&store);

        let resolution_ptr: WasmPtr<u64> = WasmPtr::new(0);
        resolution_ptr.write(&view, 100000).unwrap();

        let resolution_ref = resolution_ptr.deref(&view);
        let result = platform_clock_res_get(Snapshot0Clockid::Monotonic, resolution_ref);

        assert!(result.is_ok(), "clock_res_get(MONOTONIC) should succeed");

        let resolution_value = result.unwrap() as u64;
        resolution_ptr.write(&view, resolution_value).unwrap();

        let resolution = resolution_ptr.read(&view).unwrap();
        assert_ne!(resolution, 100000);
        assert!(resolution > 0);
        assert!(resolution <= 1_000_000_000);
    }

    #[test]
    fn test_clock_res_process_cputime() {
        let (mut store, memory) = create_test_memory();
        let view = memory.view(&store);

        let resolution_ptr: WasmPtr<u64> = WasmPtr::new(0);
        resolution_ptr.write(&view, 100000).unwrap();

        let resolution_ref = resolution_ptr.deref(&view);
        let result = platform_clock_res_get(Snapshot0Clockid::ProcessCputimeId, resolution_ref);

        assert!(
            result.is_ok(),
            "clock_res_get(PROCESS_CPUTIME_ID) should succeed"
        );

        let resolution_value = result.unwrap() as u64;
        resolution_ptr.write(&view, resolution_value).unwrap();

        let resolution = resolution_ptr.read(&view).unwrap();
        assert_ne!(resolution, 100000);
        assert!(resolution > 0);
        assert!(resolution <= 1_000_000_000);
    }

    #[test]
    fn test_clock_res_thread_cputime() {
        let (mut store, memory) = create_test_memory();
        let view = memory.view(&store);

        let resolution_ptr: WasmPtr<u64> = WasmPtr::new(0);
        resolution_ptr.write(&view, 100000).unwrap();

        let resolution_ref = resolution_ptr.deref(&view);
        let result = platform_clock_res_get(Snapshot0Clockid::ThreadCputimeId, resolution_ref);

        assert!(
            result.is_ok(),
            "clock_res_get(THREAD_CPUTIME_ID) should succeed"
        );

        let resolution_value = result.unwrap() as u64;
        resolution_ptr.write(&view, resolution_value).unwrap();

        let resolution = resolution_ptr.read(&view).unwrap();
        assert_ne!(resolution, 100000);
        assert!(resolution > 0);
        assert!(resolution <= 1_000_000_000);
    }

    #[test]
    fn test_all_clocks_valid_resolution() {
        let (mut store, memory) = create_test_memory();
        let view = memory.view(&store);

        let clocks = [
            Snapshot0Clockid::Realtime,
            Snapshot0Clockid::Monotonic,
            Snapshot0Clockid::ProcessCputimeId,
            Snapshot0Clockid::ThreadCputimeId,
        ];

        for clock_id in &clocks {
            let resolution_ptr: WasmPtr<u64> = WasmPtr::new(0);
            let resolution_ref = resolution_ptr.deref(&view);
            let result = platform_clock_res_get(*clock_id, resolution_ref);

            assert!(
                result.is_ok(),
                "clock_res_get({:?}) should succeed",
                clock_id
            );

            let resolution_value = result.unwrap() as u64;
            resolution_ptr.write(&view, resolution_value).unwrap();

            let resolution = resolution_ptr.read(&view).unwrap();
            assert!(
                resolution > 0 && resolution <= 1_000_000_000,
                "Resolution for {:?} should be between 1ns and 1s, got {}",
                clock_id,
                resolution
            );
        }
    }

    #[test]
    fn test_clock_res_consistency() {
        let (mut store, memory) = create_test_memory();
        let view = memory.view(&store);

        let ptr1: WasmPtr<u64> = WasmPtr::new(0);
        let ptr2: WasmPtr<u64> = WasmPtr::new(8);

        let ref1 = ptr1.deref(&view);
        let ref2 = ptr2.deref(&view);

        platform_clock_res_get(Snapshot0Clockid::Realtime, ref1).unwrap();
        platform_clock_res_get(Snapshot0Clockid::Realtime, ref2).unwrap();

        let res1 = ptr1.read(&view).unwrap();
        let res2 = ptr2.read(&view).unwrap();

        assert_eq!(
            res1, res2,
            "Resolution should be consistent across calls: {} vs {}",
            res1, res2
        );
    }

    #[test]
    fn test_stress_ng_repeated_calls() {
        let (mut store, memory) = create_test_memory();
        let view = memory.view(&store);

        let resolution_ptr: WasmPtr<u64> = WasmPtr::new(0);
        let resolution_ref = resolution_ptr.deref(&view);

        let mut first_resolution = 0u64;

        for i in 0..100 {
            let result = platform_clock_res_get(Snapshot0Clockid::Realtime, resolution_ref);
            assert!(
                result.is_ok(),
                "clock_res_get should succeed on iteration {}",
                i
            );

            let resolution_value = result.unwrap() as u64;
            resolution_ptr.write(&view, resolution_value).unwrap();

            let resolution = resolution_ptr.read(&view).unwrap();
            assert!(
                resolution > 0,
                "Resolution should be positive on iteration {}",
                i
            );

            if i == 0 {
                first_resolution = resolution;
            } else {
                assert_eq!(
                    resolution, first_resolution,
                    "Resolution should be consistent: iteration {} returned {}, expected {}",
                    i, resolution, first_resolution
                );
            }
        }
    }

    #[test]
    fn test_invalid_clock_id() {
        let (mut store, memory) = create_test_memory();
        let view = memory.view(&store);

        let resolution_ptr: WasmPtr<u64> = WasmPtr::new(0);
        let resolution_ref = resolution_ptr.deref(&view);

        // Unknown clock should return error
        let result = platform_clock_res_get(Snapshot0Clockid::Unknown, resolution_ref);
        assert!(
            result.is_err(),
            "clock_res_get with Unknown clock should fail"
        );
        assert_eq!(result.unwrap_err(), Errno::Inval, "Should return EINVAL");
    }
}
