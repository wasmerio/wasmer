use std::path::PathBuf;
use std::process::Command;
use std::sync::Once;
use wasmer::Module;
use wasmer_types::ModuleHash;
use wasmer_wasix::runners::wasi::{RuntimeOrEngine, WasiRunner};

static INIT: Once = Once::new();

fn setup_tracing() {
    INIT.call_once(|| {
        // Set up tracing subscriber for tracing macros
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("debug")),
            )
            .with_test_writer()
            .try_init()
            .ok();
    });
}

#[test]
fn cancel_in_context() {
    // Setup tracing and log
    setup_tracing();
    tracing::error!("Starting test: cancel_in_context");

    // Set up tokio runtime
    #[cfg(not(target_arch = "wasm32"))]
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    #[cfg(not(target_arch = "wasm32"))]
    let handle = runtime.handle().clone();
    #[cfg(not(target_arch = "wasm32"))]
    let _guard = handle.enter();

    let test_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/cancel-in-context");

    let main_c = test_dir.join("main.c");
    let main_wasm = test_dir.join("main.wasm");

    // Compile with wasixcc
    let compile_status = Command::new("wasixcc")
        .arg(&main_c)
        .arg("-sSYSROOT=/home/lennart/Documents/build-scripts/pkgs/cpython.sysroot")
        .arg("-fwasm-exceptions")
        .arg("-o")
        .arg(&main_wasm)
        .current_dir(&test_dir)
        .status()
        .expect("Failed to run wasixcc");

    assert!(compile_status.success(), "wasixcc compilation failed");

    // Load the compiled WASM module
    let wasm_bytes = std::fs::read(&main_wasm).expect("Failed to read compiled WASM file");

    let engine = wasmer::Engine::default();
    let module = Module::new(&engine, &wasm_bytes).expect("Failed to create module");

    // Run the WASM module using WasiRunner
    let runner = WasiRunner::new();
    let result = runner.run_wasm(
        RuntimeOrEngine::Engine(engine),
        "context-switching",
        module,
        ModuleHash::random(),
    );

    // Clean up
    let _ = std::fs::remove_file(&main_wasm);

    // Assert the program ran successfully
    assert!(result.is_ok(), "WASM execution failed: {:?}", result.err());
}
