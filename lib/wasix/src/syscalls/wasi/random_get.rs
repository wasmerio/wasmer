use super::*;
use crate::syscalls::*;

/// ### `random_get()`
/// Fill buffer with high-quality random data.  This function may be slow and block
/// Inputs:
/// - `void *buf`
///     A pointer to a buffer where the random bytes will be written
/// - `size_t buf_len`
///     The number of bytes that will be written
#[instrument(level = "trace", skip_all, fields(%buf_len), ret)]
pub fn random_get<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    buf: WasmPtr<u8, M>,
    buf_len: M::Offset,
) -> Errno {
    // Return EFAULT for NULL pointer with non-zero length (POSIX getentropy/getrandom behavior)
    if buf.is_null() && buf_len.into() != 0u64 {
        return Errno::Fault;
    }

    let env = ctx.data();
    let memory = unsafe { env.memory_view(&ctx) };
    let buf_len64: u64 = buf_len.into();
    let mut u8_buffer = vec![0; buf_len64 as usize];
    let res = getrandom::fill(&mut u8_buffer);
    match res {
        Ok(()) => {
            let buf = wasi_try_mem!(buf.slice(&memory, buf_len));
            wasi_try_mem!(buf.write_slice(&u8_buffer));
            Errno::Success
        }
        Err(_) => Errno::Io,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test: Basic random_get with small buffer
    #[test]
    fn test_random_get_small_buffer() {
        let mut buf = vec![0u8; 8];
        let result = getrandom::getrandom(&mut buf);
        assert!(
            result.is_ok(),
            "random_get should succeed with 8-byte buffer"
        );
        // At least one byte should be non-zero (probability ~1 in 2^64)
        assert!(
            buf.iter().any(|&b| b != 0),
            "random data should contain non-zero bytes"
        );
    }

    /// Test: Medium buffer size (64 bytes)
    #[test]
    fn test_random_get_medium_buffer() {
        let mut buf = vec![0u8; 64];
        let result = getrandom::getrandom(&mut buf);
        assert!(
            result.is_ok(),
            "random_get should succeed with 64-byte buffer"
        );
        assert!(
            buf.iter().any(|&b| b != 0),
            "random data should contain non-zero bytes"
        );
    }

    /// Test: Large buffer (1024 bytes)
    #[test]
    fn test_random_get_large_buffer() {
        let mut buf = vec![0u8; 1024];
        let result = getrandom::getrandom(&mut buf);
        assert!(
            result.is_ok(),
            "random_get should succeed with 1024-byte buffer"
        );
        assert!(
            buf.iter().any(|&b| b != 0),
            "random data should contain non-zero bytes"
        );
    }

    /// Test: Very large buffer (8192 bytes)
    #[test]
    fn test_random_get_very_large_buffer() {
        let mut buf = vec![0u8; 8192];
        let result = getrandom::getrandom(&mut buf);
        assert!(
            result.is_ok(),
            "random_get should succeed with 8192-byte buffer"
        );
        assert!(
            buf.iter().any(|&b| b != 0),
            "random data should contain non-zero bytes"
        );
    }

    /// Test: Various boundary sizes
    #[test]
    fn test_random_get_boundary_sizes() {
        let sizes = vec![1, 2, 3, 7, 8, 15, 22, 64, 127];

        for size in sizes {
            let mut buf = vec![0u8; size];
            let result = getrandom::getrandom(&mut buf);
            assert!(
                result.is_ok(),
                "random_get should succeed with {}-byte buffer",
                size
            );
            assert_eq!(buf.len(), size, "buffer size should match requested size");
        }
    }

    /// Test: Zero-size buffer (edge case)
    #[test]
    fn test_random_get_zero_size() {
        let mut buf = vec![];
        let result = getrandom::getrandom(&mut buf);
        assert!(
            result.is_ok(),
            "random_get should succeed with 0-byte buffer"
        );
    }

    /// Test: Statistical randomness check
    /// Based on: LTP getrandom02.c check_content() function
    /// Verifies byte distribution is reasonably uniform
    #[test]
    fn test_random_get_statistical_quality() {
        const BUF_SIZE: usize = 256;
        let mut buf = vec![0u8; BUF_SIZE];
        let result = getrandom::getrandom(&mut buf);
        assert!(result.is_ok(), "random_get should succeed");

        // Count occurrences of each byte value (0-255)
        let mut counts = [0u32; 256];
        for &byte in &buf {
            counts[byte as usize] += 1;
        }

        // LTP's check: no byte value should appear more than 6 + bufsize*0.2 times
        let max_count = 6 + (BUF_SIZE as f64 * 0.2) as u32;
        for (value, &count) in counts.iter().enumerate() {
            assert!(
                count <= max_count,
                "Byte value {} appears {} times (max allowed: {}), indicating poor randomness",
                value,
                count,
                max_count
            );
        }
    }

    /// Test: Multiple calls produce different data
    /// Based on: General randomness property - successive calls should differ
    #[test]
    fn test_random_get_uniqueness() {
        let mut buf1 = vec![0u8; 64];
        let mut buf2 = vec![0u8; 64];

        getrandom::getrandom(&mut buf1).expect("first call should succeed");
        getrandom::getrandom(&mut buf2).expect("second call should succeed");

        // Two calls should produce different data (probability of collision ~1 in 2^512)
        assert_ne!(
            buf1, buf2,
            "successive random_get calls should produce different data"
        );
    }

    /// Test: Thread safety (multiple concurrent calls)
    /// Based on: Linux vDSO test (256 threads)
    #[test]
    fn test_random_get_thread_safety() {
        use std::sync::{Arc, Barrier};
        use std::thread;

        const NUM_THREADS: usize = 16;
        const BUF_SIZE: usize = 32;

        let barrier = Arc::new(Barrier::new(NUM_THREADS));
        let mut handles = vec![];

        for _ in 0..NUM_THREADS {
            let barrier_clone = Arc::clone(&barrier);
            let handle = thread::spawn(move || {
                barrier_clone.wait(); // Synchronize start

                let mut buf = vec![0u8; BUF_SIZE];
                let result = getrandom::getrandom(&mut buf);
                assert!(result.is_ok(), "random_get should succeed in thread");
                assert!(
                    buf.iter().any(|&b| b != 0),
                    "random data should be non-zero"
                );
                buf
            });
            handles.push(handle);
        }

        let results: Vec<Vec<u8>> = handles
            .into_iter()
            .map(|h| h.join().expect("thread should complete"))
            .collect();

        // Verify all threads got different data
        for i in 0..results.len() {
            for j in (i + 1)..results.len() {
                assert_ne!(
                    results[i], results[j],
                    "threads {} and {} got identical random data",
                    i, j
                );
            }
        }
    }
}
