use super::*;
use crate::{journal::SnapshotTrigger, syscalls::*};

/// ### `environ_get()`
/// Read environment variable data.
/// The sizes of the buffers should match that returned by [`environ_sizes_get()`](#environ_sizes_get).
/// Inputs:
/// - `char **environ`
///     A pointer to a buffer to write the environment variable pointers.
/// - `char *environ_buf`
///     A pointer to a buffer to write the environment variable string data.
#[instrument(level = "trace", skip_all, ret)]
pub fn environ_get<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    environ: WasmPtr<WasmPtr<u8, M>, M>,
    environ_buf: WasmPtr<u8, M>,
) -> Result<Errno, WasiError> {
    ctx = wasi_try_ok!(maybe_snapshot_once::<M>(
        ctx,
        SnapshotTrigger::FirstEnviron
    )?);

    let env = ctx.data();
    let (memory, mut state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };

    let envs = state.envs.lock().unwrap();
    Ok(write_buffer_array(&memory, &envs, environ, environ_buf))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_environ_get_basic() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        // Test with two environment variables
        let builder = WasiEnv::builder("test_program")
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
    fn test_environ_get_multiple() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        // Test with multiple environment variables including empty value
        // From libc-test env.c pattern
        let builder = WasiEnv::builder("test")
            .envs([
                ("TEST1", "value1"),
                ("TEST2", "value2"),
                ("EMPTY", ""),
                ("SPECIAL", "!@#$%"),
            ])
            .engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let envs = env.state.envs.lock().unwrap();
        assert_eq!(
            *envs,
            vec![
                b"TEST1=value1".to_vec(),
                b"TEST2=value2".to_vec(),
                b"EMPTY=".to_vec(),
                b"SPECIAL=!@#$%".to_vec()
            ]
        );
    }

    #[test]
    fn test_environ_get_empty() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        // Test with no environment variables
        let builder = WasiEnv::builder("test").engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let envs = env.state.envs.lock().unwrap();
        assert_eq!(envs.len(), 0);
    }

    #[test]
    fn test_environ_get_large_count_100() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        // Test with 100 environment variables
        let mut builder = WasiEnv::builder("test");
        for i in 0..100 {
            builder = builder.env(format!("VAR{}", i), format!("value{}", i));
        }
        builder = builder.engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let envs = env.state.envs.lock().unwrap();
        assert_eq!(envs.len(), 100);

        // Verify all vars are accessible and match
        for i in 0..100 {
            assert_eq!(envs[i], format!("VAR{}=value{}", i, i).as_bytes());
        }
    }

    #[test]
    fn test_environ_get_4096_vars_stress() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        // Test: 4096 environment variables stress test
        // From go/src/syscall/env_unix_test.go pattern
        let mut builder = WasiEnv::builder("test");
        for i in 0..4096 {
            builder = builder.env(format!("DUMMY_VAR_{}", i), format!("val-{}", i * 100));
        }
        builder = builder.engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let envs = env.state.envs.lock().unwrap();
        assert_eq!(envs.len(), 4096);

        // Verify all vars accessible
        for i in 0..4096 {
            assert_eq!(
                envs[i],
                format!("DUMMY_VAR_{}=val-{}", i, i * 100).as_bytes()
            );
        }
    }

    #[test]
    fn test_environ_get_huge_value_1mb() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        // Test: Huge value (1MB)
        // From stress-ng/stress-env.c pattern
        let huge_value = "x".repeat(1024 * 1024);

        let builder = WasiEnv::builder("test")
            .env("HUGE_VAR", &huge_value)
            .engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let envs = env.state.envs.lock().unwrap();
        assert_eq!(envs.len(), 1);
        assert_eq!(envs[0], format!("HUGE_VAR={}", huge_value).as_bytes());
    }

    #[test]
    fn test_environ_get_unicode() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        // Test: Unicode values
        let builder = WasiEnv::builder("test")
            .envs([
                ("UNICODE1", "Hello 世界"),
                ("UNICODE2", "🦀 Rust"),
                ("UNICODE3", "Ñoño"),
                ("UNICODE4", "مرحبا"),
            ])
            .engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let envs = env.state.envs.lock().unwrap();
        assert_eq!(
            *envs,
            vec![
                "UNICODE1=Hello 世界".as_bytes().to_vec(),
                "UNICODE2=🦀 Rust".as_bytes().to_vec(),
                "UNICODE3=Ñoño".as_bytes().to_vec(),
                "UNICODE4=مرحبا".as_bytes().to_vec()
            ]
        );
    }

    #[test]
    fn test_environ_get_special_characters() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        // Test: Special characters in values
        // From gVisor exec.cc pattern
        let builder = WasiEnv::builder("test")
            .envs([
                ("VAR1", "special!@#$%"),
                ("VAR2", "spaces  multiple"),
                ("VAR3", "\ttab\t"),
                ("VAR4", "\nline\nbreak\n"),
                ("VAR5", "$VAR `cmd` $(cmd)"),
            ])
            .engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let envs = env.state.envs.lock().unwrap();
        assert_eq!(
            *envs,
            vec![
                b"VAR1=special!@#$%".to_vec(),
                b"VAR2=spaces  multiple".to_vec(),
                b"VAR3=\ttab\t".to_vec(),
                b"VAR4=\nline\nbreak\n".to_vec(),
                b"VAR5=$VAR `cmd` $(cmd)".to_vec()
            ]
        );
    }

    #[test]
    fn test_environ_get_very_long_key_1000_chars() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        // Test: Very long key (1000 chars)
        let long_key = "K".repeat(1000);
        let value = "value";

        let builder = WasiEnv::builder("test")
            .env(&long_key, value)
            .engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let envs = env.state.envs.lock().unwrap();
        assert_eq!(envs.len(), 1);
        assert_eq!(envs[0], format!("{}={}", long_key, value).as_bytes());
    }

    #[test]
    fn test_environ_get_empty_values() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        // Test: Multiple variables with empty values
        // From libc-test env.c clearenv pattern
        let builder = WasiEnv::builder("test")
            .envs([("VAR1", ""), ("VAR2", ""), ("VAR3", "")])
            .engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let envs = env.state.envs.lock().unwrap();
        assert_eq!(
            *envs,
            vec![b"VAR1=".to_vec(), b"VAR2=".to_vec(), b"VAR3=".to_vec()]
        );
    }

    #[test]
    fn test_environ_get_equals_in_value() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        // Test: Equals sign in value
        // From libc-test env.c putenv pattern
        let builder = WasiEnv::builder("test")
            .envs([
                ("PATH", "/bin:/usr/bin"),
                ("FORMULA", "x=y+z"),
                ("JSON", r#"{"key":"value"}"#),
            ])
            .engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let envs = env.state.envs.lock().unwrap();
        assert_eq!(
            *envs,
            vec![
                b"PATH=/bin:/usr/bin".to_vec(),
                b"FORMULA=x=y+z".to_vec(),
                br#"JSON={"key":"value"}"#.to_vec()
            ]
        );
    }

    #[test]
    fn test_environ_get_whitespace_only() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        // Test: Whitespace-only values
        let builder = WasiEnv::builder("test")
            .envs([
                ("VAR1", " "),
                ("VAR2", "  "),
                ("VAR3", "\t"),
                ("VAR4", "   \t  "),
            ])
            .engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let envs = env.state.envs.lock().unwrap();
        assert_eq!(
            *envs,
            vec![
                b"VAR1= ".to_vec(),
                b"VAR2=  ".to_vec(),
                b"VAR3=\t".to_vec(),
                b"VAR4=   \t  ".to_vec()
            ]
        );
    }
}
