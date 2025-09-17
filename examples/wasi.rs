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
    runners::wasi::{RuntimeOrEngine, WasiRunner},
    runtime::task_manager::tokio::TokioTaskManager,
    Pipe, PluggableRuntime, Runtime,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let wasm_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/wasi-wast/wasi/unstable/hello.wasm"
    );
    // Let's declare the Wasm module with the text representation.
    let wasm_bytes = std::fs::read(wasm_path)?;

    // We optionally need a tokio runtime and a WASI runtime. This doesn't need to
    // happen though; see the wasi-pipes example for an alternate approach. Things
    // such as the file system or networking can be configured on the runtime.
    let tokio_runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let _guard = tokio_runtime.enter();
    let tokio_task_manager = TokioTaskManager::new(tokio_runtime.handle().clone());
    let runtime = PluggableRuntime::new(Arc::new(tokio_task_manager));

    println!("Compiling module...");
    // Let's compile the Wasm module.
    let data = wasmer_wasix::runtime::module_cache::HashedModuleData::new_sha256(wasm_bytes);
    let module = runtime.load_module_sync(data)?;

    // Create a pipe for the module's stdout.
    let (stdout_tx, mut stdout_rx) = Pipe::channel();

    {
        // Create a WASI runner. We use a scope to make sure the runner is dropped
        // as soon as we are done with it; otherwise, it will keep the stdout pipe
        // open.
        let mut runner = WasiRunner::new();
        runner.with_stdout(Box::new(stdout_tx));

        println!("Running module...");
        // Now, run the module.
        runner.run_wasm(
            RuntimeOrEngine::Runtime(Arc::new(runtime)),
            "hello",
            module,
            wasmer_types::ModuleHash::xxhash(wasm_bytes),
        )?;
    }

    println!("Run complete - reading output");

    let mut buf = String::new();
    stdout_rx.read_to_string(&mut buf).unwrap();

    println!("Output: {buf}");

    // Verify the module wrote the correct thing, for the test below
    assert_eq!(buf, "Hello, world!\n");

    Ok(())
}

#[test]
#[cfg(feature = "wasi")]
fn test_wasi() -> Result<(), Box<dyn std::error::Error>> {
    main()
}
