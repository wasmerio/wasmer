use virtual_fs::{AsyncReadExt, AsyncWriteExt};
use virtual_mio::InlineWaker;
use wasmer::Module;
use wasmer_types::ModuleHash;
use wasmer_wasix::{
    runners::wasi::{RuntimeOrEngine, WasiRunner},
    Pipe,
};

mod sys {
    #[test]
    fn test_stdout() {
        super::test_stdout();
    }

    #[test]
    fn test_stdin() {
        super::test_stdin();
    }

    #[test]
    fn test_env() {
        super::test_env();
    }
}

// #[cfg(feature = "js")]
// mod js {
//     use wasm_bindgen_test::*;

//     #[wasm_bindgen_test]
//     fn test_stdout() {
//         super::test_stdout();
//     }

//     #[wasm_bindgen_test]
//     fn test_stdin() {
//         super::test_stdin();
//     }

//     #[wasm_bindgen_test]
//     fn test_env() {
//         super::test_env();
//     }
// }

fn test_stdout() {
    #[cfg(not(target_arch = "wasm32"))]
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    #[cfg(not(target_arch = "wasm32"))]
    let handle = runtime.handle().clone();
    #[cfg(not(target_arch = "wasm32"))]
    let _guard = handle.enter();

    let engine = wasmer::Engine::default();
    let module = Module::new(&engine, br#"
    (module
        ;; Import the required fd_write WASI function which will write the given io vectors to stdout
        ;; The function signature for fd_write is:
        ;; (File Descriptor, *iovs, iovs_len, nwritten) -> Returns number of bytes written
        (import "wasi_unstable" "fd_write" (func $fd_write (param i32 i32 i32 i32) (result i32)))

        (memory 1)
        (export "memory" (memory 0))

        ;; Write 'hello world\n' to memory at an offset of 8 bytes
        ;; No trailing newline is required since all stdio writes are flushing
        (data (i32.const 8) "hello world")

        (func $main (export "_start")
            ;; Creating a new io vector within linear memory
            (i32.store (i32.const 0) (i32.const 8))  ;; iov.iov_base - This is a pointer to the start of the 'hello world\n' string
            (i32.store (i32.const 4) (i32.const 11))  ;; iov.iov_len - The length of the 'hello world\n' string

            (call $fd_write
                (i32.const 1) ;; file_descriptor - 1 for stdout
                (i32.const 0) ;; *iovs - The pointer to the iov array, which is stored at memory location 0
                (i32.const 1) ;; iovs_len - We're printing 1 string stored in an iov - so one.
                (i32.const 20) ;; nwritten - A place in memory to store the number of bytes written
            )
            drop ;; Discard the number of bytes written from the top of the stack
        )
    )
    "#).unwrap();

    // Create the `WasiEnv`.
    let (stdout_tx, mut stdout_rx) = Pipe::channel();

    {
        let mut runner = WasiRunner::new();
        runner
            .with_stdout(Box::new(stdout_tx))
            .with_args(["Gordon"]);

        runner
            .run_wasm(
                RuntimeOrEngine::Engine(engine),
                "command-name",
                module,
                ModuleHash::random(),
            )
            .unwrap();
    }

    let mut stdout_str = String::new();
    InlineWaker::block_on(stdout_rx.read_to_string(&mut stdout_str)).unwrap();
    let stdout_as_str = stdout_str.as_str();
    assert_eq!(stdout_as_str, "hello world");
}

fn test_env() {
    #[cfg(not(target_arch = "wasm32"))]
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    #[cfg(not(target_arch = "wasm32"))]
    let handle = runtime.handle().clone();
    #[cfg(not(target_arch = "wasm32"))]
    let _guard = handle.enter();

    let engine = wasmer::Engine::default();
    let module = Module::new(&engine, include_bytes!("envvar.wasm")).unwrap();

    #[cfg(feature = "js")]
    tracing_wasm::set_as_global_default_with_config({
        let mut builder = tracing_wasm::WASMLayerConfigBuilder::new();
        builder.set_console_config(tracing_wasm::ConsoleConfig::ReportWithoutConsoleColor);
        builder.build()
    });

    // Create the `WasiEnv`.
    let (pipe_tx, mut pipe_rx) = Pipe::channel();

    {
        let mut runner = WasiRunner::new();
        runner
            .with_stdout(Box::new(pipe_tx))
            .with_args(["Gordon"])
            .with_envs([("DOG", "X"), ("TEST", "VALUE"), ("TEST2", "VALUE2")]);

        runner
            .run_wasm(
                RuntimeOrEngine::Engine(engine),
                "command-name",
                module,
                ModuleHash::random(),
            )
            .unwrap();
    }

    let mut stdout_str = String::new();
    InlineWaker::block_on(pipe_rx.read_to_string(&mut stdout_str)).unwrap();
    let stdout_as_str = stdout_str.as_str();
    assert_eq!(stdout_as_str, "Env vars:\nDOG=X\nTEST2=VALUE2\nTEST=VALUE\nDOG Ok(\"X\")\nDOG_TYPE Err(NotPresent)\nSET VAR Ok(\"HELLO\")\n");
}

fn test_stdin() {
    #[cfg(not(target_arch = "wasm32"))]
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    #[cfg(not(target_arch = "wasm32"))]
    let handle = runtime.handle().clone();
    #[cfg(not(target_arch = "wasm32"))]
    let _guard = handle.enter();

    let engine = wasmer::Engine::default();
    let module = Module::new(&engine, include_bytes!("stdin-hello.wasm")).unwrap();

    // Create the `WasiEnv`.
    let (mut pipe_tx, pipe_rx) = Pipe::channel();

    // Write to STDIN
    let buf = "Hello, stdin!\n".as_bytes().to_owned();
    InlineWaker::block_on(pipe_tx.write_all(&buf[..])).unwrap();

    {
        let mut runner = WasiRunner::new();
        runner.with_stdin(Box::new(pipe_rx));

        runner
            .run_wasm(
                RuntimeOrEngine::Engine(engine),
                "command-name",
                module,
                ModuleHash::random(),
            )
            .unwrap();
    }

    // We assure stdin is now empty
    // Can't easily be tested with current pipe impl.
    // let mut buf = Vec::new();
    // pipe.read_to_end(&mut buf).await.unwrap();
    // assert_eq!(buf.len(), 0);
}
