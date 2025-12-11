use std::path::PathBuf;
use std::process::Command;
use wasmer::Module;
use wasmer_types::ModuleHash;
use wasmer_wasix::runners::wasi::{RuntimeOrEngine, WasiRunner};

fn test_with_wasixcc(name: &str) -> Result<(), anyhow::Error> {
    eprintln!("Compiling test case: {}", name);
    let test_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join(PathBuf::from(
            file!().split('/').last().unwrap().trim_end_matches(".rs"),
        ));
    let main_c = test_dir.join(format!("{name}.c"));
    let main_wasm = test_dir.join(format!("{name}.test.wasm"));

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
    runner.run_wasm(
        RuntimeOrEngine::Engine(engine),
        "wasix-test",
        module,
        ModuleHash::random(),
    )
}

#[cfg(target_os = "linux")]
#[test]
fn test_simple_switching() {
    test_with_wasixcc("simple_switching").unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn test_switching_with_main() {
    test_with_wasixcc("switching_with_main").unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn test_switching_to_a_deleted_context() {
    test_with_wasixcc("switching_to_a_deleted_context").unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn test_switching_threads() {
    test_with_wasixcc("switching_in_threads").unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn test_multiple_contexts() {
    test_with_wasixcc("multiple_contexts").unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn test_error_handling() {
    test_with_wasixcc("error_handling").unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn test_nested_switches() {
    test_with_wasixcc("nested_switches").unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn test_contexts_with_mutexes() {
    test_with_wasixcc("contexts_with_mutexes").unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn test_contexts_with_env_vars() {
    test_with_wasixcc("contexts_with_env_vars").unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn test_contexts_with_signals() {
    test_with_wasixcc("contexts_with_signals").unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn test_contexts_with_timers() {
    test_with_wasixcc("contexts_with_timers").unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn test_contexts_with_pipes() {
    test_with_wasixcc("contexts_with_pipes").unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn test_pending_file_operations() {
    test_with_wasixcc("pending_file_operations").unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn test_recursive_host_calls() {
    test_with_wasixcc("recursive_host_calls").unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn test_malloc_during_switch() {
    test_with_wasixcc("malloc_during_switch").unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn test_nested_host_call_switch() {
    test_with_wasixcc("nested_host_call_switch").unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn test_switch_to_never_resumed() {
    test_with_wasixcc("switch_to_never_resumed").unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn test_three_way_recursion() {
    test_with_wasixcc("three_way_recursion").unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn test_contexts_with_setjmp() {
    test_with_wasixcc("contexts_with_setjmp").unwrap();
}
