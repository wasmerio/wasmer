//! Piping to and from a WASI compiled WebAssembly module with Wasmer.
//!
//! This example builds on the WASI example, showing how you can pipe to and
//! from a WebAssembly module.
//!
//! You can run the example directly by executing in Wasmer root:
//!
//! ```shell
//! cargo run --example wasi-pipes --release --features "cranelift,tokio,backend,wasi"
//! ```
//!
//! Ready?

use std::io::{Read, Write};

use wasmer::Module;
use wasmer_wasix::{
    Pipe,
    runners::wasi::{RuntimeOrEngine, WasiRunner},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let wasm_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/wasi-wast/wasi/unstable/pipe_reverse.wasm"
    );
    // Let's declare the Wasm module with the text representation.
    let wasm_bytes = std::fs::read(wasm_path)?;

    // We need at least an engine to be able to compile the module.
    let engine = wasmer::Engine::default();

    println!("Compiling module...");
    // Let's compile the Wasm module.
    let module = Module::new(&engine, &wasm_bytes[..])?;

    let msg = "racecar go zoom";
    println!("Writing \"{msg}\" to the WASI stdin...");
    let (mut stdin_sender, stdin_reader) = Pipe::channel();
    let (stdout_sender, mut stdout_reader) = Pipe::channel();

    // To write to the stdin
    writeln!(stdin_sender, "{msg}")?;

    {
        // Create a WASI runner. We use a scope to make sure the runner is dropped
        // as soon as we are done with it; otherwise, it will keep the stdout pipe
        // open.
        let mut runner = WasiRunner::new();

        // Configure the WasiRunner with the stdio pipes.
        runner
            .with_stdin(Box::new(stdin_reader))
            .with_stdout(Box::new(stdout_sender));

        // Now, run the module.
        println!("Running module...");
        runner.run_wasm(
            RuntimeOrEngine::Engine(engine),
            "hello",
            module,
            wasmer_types::ModuleHash::new(wasm_bytes),
        )?;
    }

    // To read from the stdout
    let mut buf = String::new();
    stdout_reader.read_to_string(&mut buf)?;
    println!("Read \"{}\" from the WASI stdout!", buf.trim());

    // Verify the module wrote the correct thing, for the test below
    assert_eq!(buf.trim(), "mooz og racecar");

    Ok(())
}

#[test]
#[cfg(feature = "wasi")]
fn test_wasi() -> Result<(), Box<dyn std::error::Error>> {
    main()
}
