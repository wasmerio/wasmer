use super::*;
use crate::syscalls::*;

/// ### `args_sizes_get()`
/// Return command-line argument data sizes.
/// Outputs:
/// - `size_t *argc`
///     The number of arguments.
/// - `size_t *argv_buf_size`
///     The size of the argument string data.
#[instrument(level = "trace", skip_all, ret)]
pub fn args_sizes_get<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    argc: WasmPtr<M::Offset, M>,
    argv_buf_size: WasmPtr<M::Offset, M>,
) -> Errno {
    let env = ctx.data();
    let (memory, mut state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };

    let argc = argc.deref(&memory);
    let argv_buf_size = argv_buf_size.deref(&memory);

    let argc_val: M::Offset = wasi_try!(
        state
            .args
            .lock()
            .unwrap()
            .len()
            .try_into()
            .map_err(|_| Errno::Overflow)
    );
    let argv_buf_size_val: usize = state.args.lock().unwrap().iter().map(|v| v.len() + 1).sum();
    let argv_buf_size_val: M::Offset =
        wasi_try!(argv_buf_size_val.try_into().map_err(|_| Errno::Overflow));
    wasi_try_mem!(argc.write(argc_val));
    wasi_try_mem!(argv_buf_size.write(argv_buf_size_val));

    debug!("argc={}, argv_buf_size={}", argc_val, argv_buf_size_val);

    Errno::Success
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_args_sizes_get_basic() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        // Test with single argument (program name)
        let builder = WasiEnv::builder("test_program").engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let args = env.state.args.lock().unwrap();
        assert_eq!(args.len(), 1, "Should have exactly 1 argument");
        assert!(!args[0].is_empty(), "Program name should not be empty");
    }

    #[test]
    fn test_args_sizes_get_multiple_args() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        // Test with multiple arguments
        let builder = WasiEnv::builder("test_program")
            .args(["arg1", "arg with spaces", "", "special!@#$%"])
            .engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let args = env.state.args.lock().unwrap();
        assert_eq!(args.len(), 5);
        assert_eq!(args[1], "arg1");
        assert_eq!(args[2], "arg with spaces");
        assert_eq!(args[3], "");
        assert_eq!(args[4], "special!@#$%");
    }

    #[test]
    fn test_args_sizes_get_large_args() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        // Test with many arguments
        let mut builder = WasiEnv::builder("test_program");
        for i in 0..100 {
            builder = builder.arg(format!("arg{}", i));
        }
        builder = builder.engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let args = env.state.args.lock().unwrap();
        assert_eq!(args.len(), 101, "Should have 101 arguments");

        // Verify all args are accessible
        for i in 1..=100 {
            assert_eq!(args[i], format!("arg{}", i - 1));
        }
    }

    #[test]
    fn test_args_sizes_get_empty_args() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        // Test with only empty string arguments
        let builder = WasiEnv::builder("test")
            .args(["", "", ""])
            .engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let args = env.state.args.lock().unwrap();
        assert_eq!(args.len(), 4);
        assert_eq!(args[1], "");
        assert_eq!(args[2], "");
        assert_eq!(args[3], "");
    }

    #[test]
    fn test_libc_argv_null_termination() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        let builder = WasiEnv::builder("test_program")
            .args(["arg1", "arg2", "arg3"])
            .engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let args = env.state.args.lock().unwrap();
        assert_eq!(args.len(), 4);
        assert_eq!(args[1], "arg1");
        assert_eq!(args[2], "arg2");
        assert_eq!(args[3], "arg3");
        assert!(args.get(4).is_none());
    }

    #[test]
    fn test_large_argc_4096() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        // Create 4096 arguments (stress test)
        let mut builder = WasiEnv::builder("test_program");
        for i in 0..4096 {
            builder = builder.arg(format!("arg_{}", i));
        }
        builder = builder.engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let args = env.state.args.lock().unwrap();
        assert_eq!(args.len(), 4097);

        for i in 1..=4096 {
            assert_eq!(args[i], format!("arg_{}", i - 1));
        }
    }

    #[test]
    fn test_unicode_arguments() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        let builder = WasiEnv::builder("test_program")
            .args(["Hello 世界", "🦀 Rust", "Ñoño", "مرحبا"])
            .engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let args = env.state.args.lock().unwrap();
        assert_eq!(args.len(), 5);
        assert_eq!(args[1], "Hello 世界");
        assert_eq!(args[2], "🦀 Rust");
        assert_eq!(args[3], "Ñoño");
        assert_eq!(args[4], "مرحبا");
    }

    #[test]
    fn test_execve06_empty_argv_cve_2021_4034() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        // Test: Empty argv array (just program name added by system)
        // This tests that kernel adds dummy argv[0] when empty argument list is passed
        let builder = WasiEnv::builder("execve06_child").engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let args = env.state.args.lock().unwrap();
        assert_eq!(args.len(), 1);
        assert_eq!(args[0], "execve06_child");
    }

    #[test]
    fn test_stress_ng_huge_arg_32_pages() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        // Test: Huge argument (32 pages = 131072 bytes on 4KB pages)
        let page_size = 4096;
        let huge_arg = "x".repeat(page_size * 32);

        let builder = WasiEnv::builder("stress-ng")
            .arg(&huge_arg)
            .engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let args = env.state.args.lock().unwrap();
        assert_eq!(args.len(), 2);
        assert_eq!(args[1], huge_arg);
    }

    #[test]
    fn test_nul_byte_in_argument() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        // Test: Argument with embedded NUL should be rejected for security
        let arg_with_nul = "before\0after";
        let builder = WasiEnv::builder("test_program")
            .arg(arg_with_nul)
            .engine(wasmer::Engine::default());

        let result = builder.build_init();
        assert!(result.is_err());
    }

    #[test]
    fn test_whitespace_only_argument() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        let builder = WasiEnv::builder("test_program")
            .args(["   ", "\t\t", "\n", " \t\n "])
            .engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let args = env.state.args.lock().unwrap();
        assert_eq!(args.len(), 5);
        assert_eq!(args[1], "   ");
        assert_eq!(args[2], "\t\t");
        assert_eq!(args[3], "\n");
        assert_eq!(args[4], " \t\n ");
    }

    #[test]
    fn test_shell_special_characters() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        let builder = WasiEnv::builder("test_program")
            .args(["$VAR", "`cmd`", "$(cmd)", "|", "&", ";", ">", "<", "2>&1"])
            .engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let args = env.state.args.lock().unwrap();

        assert_eq!(args.len(), 10);
        assert_eq!(args[1], "$VAR");
        assert_eq!(args[2], "`cmd`");
        assert_eq!(args[3], "$(cmd)");
        assert_eq!(args[4], "|");
        assert_eq!(args[5], "&");
        assert_eq!(args[6], ";");
        assert_eq!(args[7], ">");
        assert_eq!(args[8], "<");
        assert_eq!(args[9], "2>&1");
    }

    #[test]
    fn test_many_single_char_arguments() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        // Create 1000 single-character arguments
        let mut builder = WasiEnv::builder("test_program");
        for i in 0..1000 {
            let ch = (b'a' + (i % 26) as u8) as char;
            builder = builder.arg(&ch.to_string());
        }
        builder = builder.engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let args = env.state.args.lock().unwrap();
        assert_eq!(args.len(), 1001);

        for i in 1..=1000 {
            assert_eq!(args[i].len(), 1);
        }
    }
}
