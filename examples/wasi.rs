//! Running a WASI compiled WebAssembly module with Wasmer.
//!
//! This example illustrates how to run WASI modules with
//! Wasmer.
//!
//! If you need more manual control over the instantiation, including custom
//! imports, then check out the ./wasi_manual_setup.rs example.
//!
//! You can run the example directly by executing in Wasmer root:
//!
//! ```shell
//! cargo run --example wasi --release --features "cranelift,wasi"
//! ```
//!
//! Ready?

use std::{io::Read, sync::Arc};

use wasmer_wasix::{
    runners::wasi::WasiRunner, runtime::task_manager::tokio::TokioTaskManager, Pipe,
    PluggableRuntime, Runtime,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let wasm_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/wasi-wast/wasi/unstable/hello.wasm"
    );
    // Let's declare the Wasm module with the text representation.
    let wasm_bytes = std::fs::read(wasm_path)?;

    // We need a WASI runtime.
    let tokio_task_manager = TokioTaskManager::new(tokio::runtime::Handle::current());
    let runtime = PluggableRuntime::new(Arc::new(tokio_task_manager));

    println!("Compiling module...");
    // Let's compile the Wasm module.
    let module = runtime.load_module(&wasm_bytes[..]).await?;

    // Create a WASI runner.
    let mut runner = WasiRunner::new();

    // Create a pipe for the module's stdout.
    let (stdout_tx, mut stdout_rx) = Pipe::channel();
    runner.with_stdout(Box::new(stdout_tx));

    // Now, run the module.
    runner.run_wasm(
        Arc::new(runtime),
        "hello",
        module,
        wasmer_types::ModuleHash::xxhash(wasm_bytes),
    )?;

    eprintln!("Run complete - reading output");

    let mut buf = String::new();
    stdout_rx.read_to_string(&mut buf).unwrap();

    eprintln!("Output: {buf}");

    Ok(())
}

#[test]
#[cfg(feature = "wasi")]
fn test_wasi() -> Result<(), Box<dyn std::error::Error>> {
    main()
}
