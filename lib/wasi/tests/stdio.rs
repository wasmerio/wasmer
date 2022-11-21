use std::io::{Read, Write};

use wasmer::{Instance, Module, Store};
use wasmer_wasi::{Pipe, WasiState};

mod sys {
    #[test]
    fn test_stdout() {
        super::test_stdout()
    }

    #[test]
    fn test_stdin() {
        super::test_stdin()
    }

    #[test]
    fn test_env() {
        super::test_env()
    }
}

#[cfg(feature = "js")]
mod js {
    use wasm_bindgen_test::*;

    #[wasm_bindgen_test]
    fn test_stdout() {
        super::test_stdout()
    }

    #[wasm_bindgen_test]
    fn test_stdin() {
        super::test_stdin()
    }

    #[wasm_bindgen_test]
    fn test_env() {
        super::test_env()
    }
}

fn test_stdout() {
    let mut store = Store::default();
    let module = Module::new(&mut store, br#"
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
    let mut stdout = Pipe::default();
    let wasi_env = WasiState::new("command-name")
        .args(&["Gordon"])
        .stdout(Box::new(stdout.clone()))
        .finalize(&mut store)
        .unwrap();

    // Generate an `ImportObject`.
    let import_object = wasi_env.import_object(&mut store, &module).unwrap();

    // Let's instantiate the module with the imports.
    let instance = Instance::new(&mut store, &module, &import_object).unwrap();
    let memory = instance.exports.get_memory("memory").unwrap();
    wasi_env.data_mut(&mut store).set_memory(memory.clone());

    // Let's call the `_start` function, which is our `main` function in Rust.
    let start = instance.exports.get_function("_start").unwrap();
    start.call(&mut store, &[]).unwrap();

    let mut stdout_str = String::new();
    stdout.read_to_string(&mut stdout_str).unwrap();
    let stdout_as_str = stdout_str.as_str();
    assert_eq!(stdout_as_str, "hello world\n");
}

fn test_env() {
    let mut store = Store::default();
    let module = Module::new(&store, include_bytes!("envvar.wasm")).unwrap();

    #[cfg(feature = "js")]
    tracing_wasm::set_as_global_default_with_config({
        let mut builder = tracing_wasm::WASMLayerConfigBuilder::new();
        builder.set_console_config(tracing_wasm::ConsoleConfig::ReportWithoutConsoleColor);
        builder.build()
    });

    // Create the `WasiEnv`.
    let mut stdout = Pipe::new();
    let mut wasi_state_builder = WasiState::new("command-name");
    wasi_state_builder
        .args(&["Gordon"])
        .env("DOG", "X")
        .env("TEST", "VALUE")
        .env("TEST2", "VALUE2");
    // panic!("envs: {:?}", wasi_state_builder.envs);
    let wasi_env = wasi_state_builder
        .stdout(Box::new(stdout.clone()))
        .finalize(&mut store)
        .unwrap();

    // Generate an `ImportObject`.
    let import_object = wasi_env.import_object(&mut store, &module).unwrap();

    // Let's instantiate the module with the imports.
    let instance = Instance::new(&mut store, &module, &import_object).unwrap();
    let memory = instance.exports.get_memory("memory").unwrap();
    wasi_env.data_mut(&mut store).set_memory(memory.clone());

    // Let's call the `_start` function, which is our `main` function in Rust.
    let start = instance.exports.get_function("_start").unwrap();
    start.call(&mut store, &[]).unwrap();

    let mut stdout_str = String::new();
    stdout.read_to_string(&mut stdout_str).unwrap();
    let stdout_as_str = stdout_str.as_str();
    assert_eq!(stdout_as_str, "Env vars:\nDOG=X\nTEST2=VALUE2\nTEST=VALUE\nDOG Ok(\"X\")\nDOG_TYPE Err(NotPresent)\nSET VAR Ok(\"HELLO\")\n");
}

fn test_stdin() {
    let mut store = Store::default();
    let module = Module::new(&store, include_bytes!("stdin-hello.wasm")).unwrap();

    // Create the `WasiEnv`.
    let mut stdin = Pipe::new();
    let wasi_env = WasiState::new("command-name")
        .stdin(Box::new(stdin.clone()))
        .finalize(&mut store)
        .unwrap();

    // Write to STDIN
    let buf = "Hello, stdin!\n".as_bytes().to_owned();
    stdin.write(&buf[..]).unwrap();

    // Generate an `ImportObject`.
    let import_object = wasi_env.import_object(&mut store, &module).unwrap();

    // Let's instantiate the module with the imports.
    let instance = Instance::new(&mut store, &module, &import_object).unwrap();
    let memory = instance.exports.get_memory("memory").unwrap();
    wasi_env.data_mut(&mut store).set_memory(memory.clone());

    // Let's call the `_start` function, which is our `main` function in Rust.
    let start = instance.exports.get_function("_start").unwrap();
    let result = start.call(&mut store, &[]);
    assert!(!result.is_err());

    // We assure stdin is now empty
    let mut buf = Vec::new();
    stdin.read_to_end(&mut buf).unwrap();
    assert_eq!(buf.len(), 0);
}
