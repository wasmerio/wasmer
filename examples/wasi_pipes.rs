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

use std::{
    io::{Read, Write},
    sync::Arc,
};

use wasmer_wasix::{
    runners::wasi::WasiRunner, runtime::task_manager::tokio::TokioTaskManager, Pipe,
    PluggableRuntime, Runtime,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let wasm_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/wasi-wast/wasi/unstable/pipe_reverse.wasm"
    );
    // Let's declare the Wasm module with the text representation.
    let wasm_bytes = std::fs::read(wasm_path)?;

    // We need a WASI runtime.
    let tokio_task_manager = TokioTaskManager::new(tokio::runtime::Handle::current());
    let runtime = PluggableRuntime::new(Arc::new(tokio_task_manager));

    println!("Compiling module...");
    // Let's compile the Wasm module.
    let module = runtime.load_module(&wasm_bytes[..]).await?;

    let msg = "racecar go zoom";
    println!("Writing \"{}\" to the WASI stdin...", msg);
    let (mut stdin_sender, stdin_reader) = Pipe::channel();
    let (stdout_sender, mut stdout_reader) = Pipe::channel();

    // To write to the stdin
    writeln!(stdin_sender, "{}", msg)?;

    // Create a WASI runner.
    let mut runner = WasiRunner::new();

    // Configure the WasiRunner with the stdio pipes.
    runner
        .with_stdin(Box::new(stdin_reader))
        .with_stdout(Box::new(stdout_sender));

    // Now, run the module.
    println!("Running module...");
    runner.run_wasm(
        Arc::new(runtime),
        "hello",
        module,
        wasmer_types::ModuleHash::xxhash(wasm_bytes),
    )?;

    // To read from the stdout
    let mut buf = String::new();
    stdout_reader.read_to_string(&mut buf)?;
    println!("Read \"{}\" from the WASI stdout!", buf.trim());

    Ok(())
}

#[test]
#[cfg(feature = "wasi")]
fn test_wasi() -> Result<(), Box<dyn std::error::Error>> {
    main()
}
