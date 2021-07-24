#![cfg(feature = "js")]

use wasm_bindgen_test::*;
use wasmer::{Instance, Module, Store};
use wasmer_wasi::{Stdin, Stdout, WasiState};

#[wasm_bindgen_test]
fn test_stdout() {
    let store = Store::default();
    let module = Module::new(&store, br#"
    (module
        ;; Import the required fd_write WASI function which will write the given io vectors to stdout
        ;; The function signature for fd_write is:
        ;; (File Descriptor, *iovs, iovs_len, nwritten) -> Returns number of bytes written
        (import "wasi_unstable" "fd_write" (func $fd_write (param i32 i32 i32 i32) (result i32)))

        (memory 1)
        (export "memory" (memory 0))

        ;; Write 'hello world\n' to memory at an offset of 8 bytes
        ;; Note the trailing newline which is required for the text to appear
        (data (i32.const 8) "hello world\n")

        (func $main (export "_start")
            ;; Creating a new io vector within linear memory
            (i32.store (i32.const 0) (i32.const 8))  ;; iov.iov_base - This is a pointer to the start of the 'hello world\n' string
            (i32.store (i32.const 4) (i32.const 12))  ;; iov.iov_len - The length of the 'hello world\n' string

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
    // let stdout = Stdout::default();
    let mut wasi_env = WasiState::new("command-name")
        .args(&["Gordon"])
        // .stdout(Box::new(stdout))
        .finalize()
        .unwrap();

    // Generate an `ImportObject`.
    let import_object = wasi_env.import_object(&module).unwrap();

    // Let's instantiate the module with the imports.
    let instance = Instance::new(&module, &import_object).unwrap();

    // Let's call the `_start` function, which is our `main` function in Rust.
    let start = instance.exports.get_function("_start").unwrap();
    start.call(&[]).unwrap();

    let state = wasi_env.state();
    let stdout = state.fs.stdout().unwrap().as_ref().unwrap();
    let stdout = stdout.downcast_ref::<Stdout>().unwrap();
    let stdout_as_str = std::str::from_utf8(&stdout.buf).unwrap();
    assert_eq!(stdout_as_str, "hello world\n");
}
#[wasm_bindgen_test]
fn test_env() {
    let store = Store::default();
    let module = Module::new(&store, include_bytes!("envvar.wasm")).unwrap();

    tracing_wasm::set_as_global_default_with_config({
        let mut builder = tracing_wasm::WASMLayerConfigBuilder::new();
        builder.set_console_config(tracing_wasm::ConsoleConfig::ReportWithoutConsoleColor);
        builder.build()
    });

    // Create the `WasiEnv`.
    // let stdout = Stdout::default();
    let mut wasi_state_builder = WasiState::new("command-name");
    wasi_state_builder
        .args(&["Gordon"])
        .env("DOG", "X")
        .env("TEST", "VALUE")
        .env("TEST2", "VALUE2");
    // panic!("envs: {:?}", wasi_state_builder.envs);
    let mut wasi_env = wasi_state_builder
        // .stdout(Box::new(stdout))
        .finalize()
        .unwrap();

    // Generate an `ImportObject`.
    let import_object = wasi_env.import_object(&module).unwrap();

    // Let's instantiate the module with the imports.
    let instance = Instance::new(&module, &import_object).unwrap();

    // Let's call the `_start` function, which is our `main` function in Rust.
    let start = instance.exports.get_function("_start").unwrap();
    start.call(&[]).unwrap();

    let state = wasi_env.state();
    let stdout = state.fs.stdout().unwrap().as_ref().unwrap();
    let stdout = stdout.downcast_ref::<Stdout>().unwrap();
    let stdout_as_str = std::str::from_utf8(&stdout.buf).unwrap();
    assert_eq!(stdout_as_str, "Env vars:\nDOG=X\nTEST2=VALUE2\nTEST=VALUE\nDOG Ok(\"X\")\nDOG_TYPE Err(NotPresent)\nSET VAR Ok(\"HELLO\")\n");
}

#[wasm_bindgen_test]
fn test_stdin() {
    let store = Store::default();
    let module = Module::new(&store, include_bytes!("stdin-hello.wasm")).unwrap();

    // Create the `WasiEnv`.
    let mut stdin = Stdin::default();
    stdin.buf = "Hello, stdin!".as_bytes().to_owned();
    let mut wasi_env = WasiState::new("command-name")
        .stdin(Box::new(stdin))
        .finalize()
        .unwrap();

    // Generate an `ImportObject`.
    let import_object = wasi_env.import_object(&module).unwrap();

    // Let's instantiate the module with the imports.
    let instance = Instance::new(&module, &import_object).unwrap();

    // Let's call the `_start` function, which is our `main` function in Rust.
    let start = instance.exports.get_function("_start").unwrap();
    let result = start.call(&[]);
    assert!(result.is_err());
    // let status = result.unwrap_err().downcast::<WasiError>().unwrap();
    let state = wasi_env.state();
    let stdin = state.fs.stdin().unwrap().as_ref().unwrap();
    let stdin = stdin.downcast_ref::<Stdin>().unwrap();
    // We assure stdin is now empty
    assert_eq!(stdin.buf.len(), 0);
}
