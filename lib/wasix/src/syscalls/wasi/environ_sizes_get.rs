use super::*;
use crate::{journal::SnapshotTrigger, syscalls::*};

/// ### `environ_sizes_get()`
/// Return command-line argument data sizes.
/// Outputs:
/// - `size_t *environ_count`
///     The number of environment variables.
/// - `size_t *environ_buf_size`
///     The size of the environment variable string data.
#[instrument(level = "trace", skip_all, ret)]
pub fn environ_sizes_get<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    environ_count: WasmPtr<M::Offset, M>,
    environ_buf_size: WasmPtr<M::Offset, M>,
) -> Result<Errno, WasiError> {
    ctx = wasi_try_ok!(maybe_snapshot_once::<M>(
        ctx,
        SnapshotTrigger::FirstEnviron
    )?);

    let env = ctx.data();
    let (memory, mut state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };

    let environ_count = environ_count.deref(&memory);
    let environ_buf_size = environ_buf_size.deref(&memory);

    let env_var_count: M::Offset = wasi_try_ok!(
        state
            .envs
            .lock()
            .unwrap()
            .len()
            .try_into()
            .map_err(|_| Errno::Overflow)
    );
    let env_buf_size: usize = state.envs.lock().unwrap().iter().map(|v| v.len() + 1).sum();
    let env_buf_size: M::Offset =
        wasi_try_ok!(env_buf_size.try_into().map_err(|_| Errno::Overflow));
    wasi_try_mem_ok!(environ_count.write(env_var_count));
    wasi_try_mem_ok!(environ_buf_size.write(env_buf_size));

    trace!(
        %env_var_count,
        %env_buf_size
    );

    Ok(Errno::Success)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_environ_sizes_get_basic() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        let builder = WasiEnv::builder("test")
            .env("VAR1", "value1")
            .env("VAR2", "value2")
            .engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let envs = env.state.envs.lock().unwrap();
        assert_eq!(
            *envs,
            vec![b"VAR1=value1".to_vec(), b"VAR2=value2".to_vec()]
        );
    }

    #[test]
    fn test_environ_sizes_get_multiple() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        let builder = WasiEnv::builder("test")
            .env("HOME", "/home/user")
            .env("PATH", "/usr/bin:/bin")
            .env("EMPTY", "")
            .env("SPECIAL", "value with spaces!@#")
            .engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let envs = env.state.envs.lock().unwrap();
        assert_eq!(
            *envs,
            vec![
                b"HOME=/home/user".to_vec(),
                b"PATH=/usr/bin:/bin".to_vec(),
                b"EMPTY=".to_vec(),
                b"SPECIAL=value with spaces!@#".to_vec(),
            ]
        );
    }

    #[test]
    fn test_environ_sizes_get_large() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        let mut builder = WasiEnv::builder("test");
        for i in 0..100 {
            builder = builder.env(format!("VAR_{}", i), format!("value_{}", i));
        }
        builder = builder.engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let envs = env.state.envs.lock().unwrap();
        assert_eq!(envs.len(), 100);

        // Verify all vars are accessible
        for i in 0..100 {
            let expected = format!("VAR_{}=value_{}", i, i);
            assert_eq!(envs[i], expected.as_bytes());
        }
    }

    #[test]
    fn test_environ_sizes_get_empty_values() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        let builder = WasiEnv::builder("test")
            .env("EMPTY1", "")
            .env("EMPTY2", "")
            .env("NONEMPTY", "value")
            .engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let envs = env.state.envs.lock().unwrap();
        assert_eq!(
            *envs,
            vec![
                b"EMPTY1=".to_vec(),
                b"EMPTY2=".to_vec(),
                b"NONEMPTY=value".to_vec()
            ]
        );
    }

    #[test]
    fn test_environ_sizes_get_no_vars() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        let builder = WasiEnv::builder("test").engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let envs = env.state.envs.lock().unwrap();
        assert_eq!(*envs, Vec::<Vec<u8>>::new());
    }

    #[test]
    fn test_large_environ_4096() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        let mut builder = WasiEnv::builder("test");
        for i in 0..4096 {
            builder = builder.env(format!("DUMMY_VAR_{}", i), format!("val-{}", i * 100));
        }
        builder = builder.engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let envs = env.state.envs.lock().unwrap();
        assert_eq!(envs.len(), 4096);

        // Verify all variables
        for i in 0..4096 {
            let expected = format!("DUMMY_VAR_{}=val-{}", i, i * 100);
            assert_eq!(envs[i], expected.as_bytes());
        }
    }

    #[test]
    fn test_stress_huge_env_value() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        let huge_value = "x".repeat(1024 * 1024);
        let builder = WasiEnv::builder("test")
            .env("HUGE_VAR", &huge_value)
            .engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let envs = env.state.envs.lock().unwrap();
        let expected = format!("HUGE_VAR={}", huge_value);
        assert_eq!(*envs, vec![expected.as_bytes().to_vec()]);
    }

    #[test]
    fn test_libc_environ_count_validation() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        // Test: After setting TEST=1, environ should have exactly 1 variable
        let builder = WasiEnv::builder("test")
            .env("TEST", "1")
            .engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let envs = env.state.envs.lock().unwrap();
        assert_eq!(*envs, vec![b"TEST=1".to_vec()]);
    }

    #[test]
    fn test_unicode_env_values() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        let builder = WasiEnv::builder("test")
            .env("UNICODE1", "Hello 世界")
            .env("UNICODE2", "🦀 Rust")
            .env("UNICODE3", "Ñoño")
            .env("UNICODE4", "مرحبا")
            .engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let envs = env.state.envs.lock().unwrap();
        assert_eq!(
            *envs,
            vec![
                "UNICODE1=Hello 世界".as_bytes().to_vec(),
                "UNICODE2=🦀 Rust".as_bytes().to_vec(),
                "UNICODE3=Ñoño".as_bytes().to_vec(),
                "UNICODE4=مرحبا".as_bytes().to_vec(),
            ]
        );
    }

    #[test]
    fn test_very_long_env_key() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        let long_key = "VAR_".to_string() + &"X".repeat(996); // 1000 chars total
        let builder = WasiEnv::builder("test")
            .env(&long_key, "value")
            .engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let envs = env.state.envs.lock().unwrap();
        let expected = format!("{}=value", long_key);
        assert_eq!(*envs, vec![expected.as_bytes().to_vec()]);
    }
}
