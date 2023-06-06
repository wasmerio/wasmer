use virtual_fs::{AsyncReadExt, AsyncWriteExt};
use wasmer::{Module, Store};
use wasmer_wasix::{Pipe, WasiEnv};

mod sys {
    #[tokio::test]
    async fn test_stdout() {
        super::test_stdout().await;
    }

    #[tokio::test]
    async fn test_stdin() {
        super::test_stdin().await;
    }

    #[tokio::test]
    async fn test_env() {
        super::test_env().await;
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

async fn test_stdout() {
    let mut store = Store::default();
    let module = Module::new(&store, br#"
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

    let builder = WasiEnv::builder("command-name")
        .args(["Gordon"])
        .stdout(Box::new(stdout_tx));

    #[cfg(feature = "js")]
    {
        builder.run_with_store(module, &mut store).unwrap();
    }
    #[cfg(not(feature = "js"))]
    {
        std::thread::spawn(move || builder.run_with_store(module, &mut store))
            .join()
            .unwrap()
            .unwrap();
    }

    let mut stdout_str = String::new();
    stdout_rx.read_to_string(&mut stdout_str).await.unwrap();
    let stdout_as_str = stdout_str.as_str();
    assert_eq!(stdout_as_str, "hello world");
}

async fn test_env() {
    let mut store = Store::default();
    let module = Module::new(&store, include_bytes!("envvar.wasm")).unwrap();

    #[cfg(feature = "js")]
    tracing_wasm::set_as_global_default_with_config({
        let mut builder = tracing_wasm::WASMLayerConfigBuilder::new();
        builder.set_console_config(tracing_wasm::ConsoleConfig::ReportWithoutConsoleColor);
        builder.build()
    });

    // Create the `WasiEnv`.
    let (pipe_tx, mut pipe_rx) = Pipe::channel();

    let builder = WasiEnv::builder("command-name")
        .args(["Gordon"])
        .env("DOG", "X")
        .env("TEST", "VALUE")
        .env("TEST2", "VALUE2")
        .stdout(Box::new(pipe_tx));

    #[cfg(feature = "js")]
    {
        builder.run_with_store(module, &mut store).unwrap();
    }

    #[cfg(not(feature = "js"))]
    {
        std::thread::spawn(move || builder.run_with_store(module, &mut store))
            .join()
            .unwrap()
            .unwrap();
    }

    let mut stdout_str = String::new();
    pipe_rx.read_to_string(&mut stdout_str).await.unwrap();
    let stdout_as_str = stdout_str.as_str();
    assert_eq!(stdout_as_str, "Env vars:\nDOG=X\nTEST2=VALUE2\nTEST=VALUE\nDOG Ok(\"X\")\nDOG_TYPE Err(NotPresent)\nSET VAR Ok(\"HELLO\")\n");
}

async fn test_stdin() {
    let mut store = Store::default();
    let module = Module::new(&store, include_bytes!("stdin-hello.wasm")).unwrap();

    // Create the `WasiEnv`.
    let (mut pipe_tx, pipe_rx) = Pipe::channel();
    // FIXME: needed? (method not available)
    // .with_blocking(false);

    // Write to STDIN
    let buf = "Hello, stdin!\n".as_bytes().to_owned();
    pipe_tx.write_all(&buf[..]).await.unwrap();

    let builder = WasiEnv::builder("command-name").stdin(Box::new(pipe_rx));

    #[cfg(feature = "js")]
    {
        builder.run_with_store(module, &mut store).unwrap();
    }

    #[cfg(not(feature = "js"))]
    {
        std::thread::spawn(move || builder.run_with_store(module, &mut store))
            .join()
            .unwrap()
            .unwrap();
    }

    // We assure stdin is now empty
    // Can't easily be tested with current pipe impl.
    // let mut buf = Vec::new();
    // pipe.read_to_end(&mut buf).await.unwrap();
    // assert_eq!(buf.len(), 0);
}
