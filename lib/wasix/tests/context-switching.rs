use std::path::PathBuf;
use std::process::Command;
use wasmer::Module;
use wasmer_types::ModuleHash;
use wasmer_wasix::runners::wasi::{RuntimeOrEngine, WasiRunner};

#[cfg(target_os = "linux")]
#[test]
fn test_context_switching() {
    let test_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join(PathBuf::from(
            file!().split('/').last().unwrap().trim_end_matches(".rs"),
        ));
    let main_c = test_dir.join("main.c");
    let main_wasm = test_dir.join("main.tmp.wasm");

    // Compile with wasixcc
    let mut command = Command::new("wasixcc");
    command
        .arg(&main_c)
        .arg("-fwasm-exceptions")
        .arg("-o")
        .arg(&main_wasm)
        .current_dir(&test_dir);
    eprintln!("Running wasixcc: {:?}", command);
    let compile_status = command.status().expect("Failed to run wasixcc");
    assert!(compile_status.success(), "wasixcc compilation failed");

    // Load the compiled WASM module
    let wasm_bytes = std::fs::read(&main_wasm).expect("Failed to read compiled WASM file");
    let engine = wasmer::Engine::default();
    let module = Module::new(&engine, &wasm_bytes).expect("Failed to create module");

    // Run the WASM module using WasiRunner
    let runner = WasiRunner::new();
    runner
        .run_wasm(
            RuntimeOrEngine::Engine(engine),
            "wasix-test",
            module,
            ModuleHash::random(),
        )
        .unwrap();
}
