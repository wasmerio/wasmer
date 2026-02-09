use super::*;
use crate::syscalls::*;

/// ### `args_get()`
/// Read command-line argument data.
/// The sizes of the buffers should match that returned by [`args_sizes_get()`](#args_sizes_get).
/// Inputs:
/// - `char **argv`
///     A pointer to a buffer to write the argument pointers.
/// - `char *argv_buf`
///     A pointer to a buffer to write the argument string data.
///
#[instrument(level = "trace", skip_all, ret)]
pub fn args_get<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    argv: WasmPtr<WasmPtr<u8, M>, M>,
    argv_buf: WasmPtr<u8, M>,
) -> Errno {
    let env = ctx.data();
    let (memory, mut state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };

    let args = state
        .args
        .lock()
        .unwrap()
        .iter()
        .map(|a| a.as_bytes().to_vec())
        .collect::<Vec<_>>();
    let result = write_buffer_array(&memory, &args, argv, argv_buf);

    debug!(
        "args:\n{}",
        state
            .args
            .lock()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(i, v)| format!("{i:>20}: {v}"))
            .collect::<Vec<String>>()
            .join("\n")
    );

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_args_get_basic() {
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
        assert_eq!(args[0], "test_program");
    }

    #[test]
    fn test_args_get_multiple_args() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        // Test with multiple arguments - validates exact string matching
        // From LTP execve01.c pattern - validates argv[1] == "canary"
        let builder = WasiEnv::builder("test_program")
            .args(["arg1", "canary", "arg with spaces", ""])
            .engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let args = env.state.args.lock().unwrap();
        assert_eq!(
            args.as_slice(),
            &["test_program", "arg1", "canary", "arg with spaces", ""]
        );
    }

    #[test]
    fn test_args_get_special_characters() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        // Test with special characters
        let builder = WasiEnv::builder("test")
            .args([
                "special!@#$%",
                "spaces  multiple",
                "\ttab\t",
                "\nline\nbreak\n",
            ])
            .engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let args = env.state.args.lock().unwrap();
        assert_eq!(
            args.as_slice(),
            &[
                "test",
                "special!@#$%",
                "spaces  multiple",
                "\ttab\t",
                "\nline\nbreak\n"
            ]
        );
    }

    #[test]
    fn test_args_get_large_count_100() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        // Test with 100 arguments
        let mut builder = WasiEnv::builder("test_program");
        for i in 0..100 {
            builder = builder.arg(format!("arg{}", i));
        }
        builder = builder.engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let args = env.state.args.lock().unwrap();
        assert_eq!(args.len(), 101);

        // Verify all args are accessible and match
        for i in 1..=100 {
            assert_eq!(args[i], format!("arg{}", i - 1));
        }
    }

    #[test]
    fn test_args_get_4096_args_stress() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        // Test: 4096 arguments stress test
        let mut builder = WasiEnv::builder("test_program");
        for i in 0..4096 {
            builder = builder.arg(format!("arg_{}", i));
        }
        builder = builder.engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let args = env.state.args.lock().unwrap();
        assert_eq!(args.len(), 4097);

        // Verify all arguments accessible
        for i in 1..=4096 {
            assert_eq!(args[i], format!("arg_{}", i - 1));
        }
    }

    #[test]
    fn test_args_get_huge_arg_131kb() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        // Test: Huge single argument (32 pages = 131072 bytes on 4KB pages)
        let page_size = 4096;
        let huge_arg = "x".repeat(page_size * 32);

        let builder = WasiEnv::builder("stress-ng")
            .arg(&huge_arg)
            .engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let args = env.state.args.lock().unwrap();
        assert_eq!(args.len(), 2);
        assert_eq!(args[1].len(), page_size * 32);
        assert_eq!(args[1], huge_arg);
    }

    #[test]
    fn test_args_get_unicode() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        // Test: Unicode arguments
        let builder = WasiEnv::builder("test_program")
            .args(["Hello 世界", "🦀 Rust", "Ñoño", "مرحبا"])
            .engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let args = env.state.args.lock().unwrap();
        assert_eq!(
            args.as_slice(),
            &["test_program", "Hello 世界", "🦀 Rust", "Ñoño", "مرحبا"]
        );
    }

    #[test]
    fn test_args_get_single_char_1000_args() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        // Test: 1000 single-character arguments
        // Tests argument parsing doesn't merge/drop arguments
        let mut builder = WasiEnv::builder("test");
        for _ in 0..1000 {
            builder = builder.arg("x");
        }
        builder = builder.engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let args = env.state.args.lock().unwrap();
        assert_eq!(args.len(), 1001);

        for i in 1..=1000 {
            assert_eq!(args[i], "x");
        }
    }

    #[test]
    fn test_args_get_whitespace_only() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        // Test: Whitespace-only arguments
        // From gVisor exec.cc InterpreterScriptTrailingWhitespace pattern
        let builder = WasiEnv::builder("test")
            .args([" ", "  ", "\t", "   \t  "])
            .engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let args = env.state.args.lock().unwrap();
        assert_eq!(args.as_slice(), &["test", " ", "  ", "\t", "   \t  "]);
    }

    #[test]
    fn test_args_get_very_long_key_1000_chars() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        // Test: Very long individual argument (1000 chars)
        let long_arg = "a".repeat(1000);

        let builder = WasiEnv::builder("test")
            .arg(&long_arg)
            .engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let args = env.state.args.lock().unwrap();
        assert_eq!(args.len(), 2);
        assert_eq!(args[1].len(), 1000);
        assert_eq!(args[1], long_arg);
    }

    #[test]
    fn test_args_get_shell_special_chars() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        // Test: Shell special characters
        // These should be passed as-is, not interpreted
        let builder = WasiEnv::builder("test")
            .args([
                "$VAR",
                "`cmd`",
                "$(cmd)",
                "|pipe|",
                "&amp",
                ";semi;",
                ">redirect>",
                "<redirect<",
            ])
            .engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let args = env.state.args.lock().unwrap();
        assert_eq!(
            args.as_slice(),
            &[
                "test",
                "$VAR",
                "`cmd`",
                "$(cmd)",
                "|pipe|",
                "&amp",
                ";semi;",
                ">redirect>",
                "<redirect<"
            ]
        );
    }

    #[test]
    fn test_args_get_argv_zero_preservation() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let _guard = handle.enter();

        // Test: argv[0] should be the program name as given
        let builder = WasiEnv::builder("./relative/path/program.wasm")
            .args(["arg1"])
            .engine(wasmer::Engine::default());
        let env = builder.build_init().unwrap();

        let args = env.state.args.lock().unwrap();
        assert_eq!(args.as_slice(), &["./relative/path/program.wasm", "arg1"]);
    }
}
