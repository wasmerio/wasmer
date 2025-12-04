use std::ffi::OsStr;
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
    let main_cpp = test_dir.join(format!("{name}.cpp"));
    let source = if main_c.exists() {
        main_c
    } else if main_cpp.exists() {
        main_cpp
    } else {
        anyhow::bail!("No source file found for context switching test '{name}'");
    };
    let is_cpp = source
        .extension()
        .and_then(OsStr::to_str)
        .map(|ext| ext.eq_ignore_ascii_case("cpp"))
        .unwrap_or(false);
    let main_wasm = test_dir.join(format!("{name}.test.wasm"));

    // Compile with wasixcc
    let mut command = Command::new(if is_cpp { "wasix++" } else { "wasixcc" });
    command
        .arg("-iwithsysroot")
        .arg("/usr/local/include/c++/v1")
        .arg("-iwithsysroot")
        .arg("/include/c++/v1")
        .arg("-iwithsysroot")
        .arg("/usr/include/c++/v1")
        .arg("-iwithsysroot")
        .arg("/usr/local/include")
        .arg("-iwithsysroot")
        .arg("/include")
        .arg("-iwithsysroot")
        .arg("/usr/include")
        .arg(&source)
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
fn test_state_preservation() {
    test_with_wasixcc("state_preservation").unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn test_main_context_id() {
    test_with_wasixcc("main_context_id").unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn test_self_switching() {
    test_with_wasixcc("self_switching").unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn test_cleanup_order() {
    test_with_wasixcc("cleanup_order").unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn test_deep_recursion() {
    test_with_wasixcc("deep_recursion").unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn test_rapid_switching() {
    test_with_wasixcc("rapid_switching").unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn test_heap_allocations() {
    test_with_wasixcc("heap_allocations").unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn test_many_contexts() {
    test_with_wasixcc("many_contexts").unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn test_file_io_switching() {
    test_with_wasixcc("file_io_switching").unwrap();
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
fn test_contexts_with_getcwd() {
    test_with_wasixcc("contexts_with_getcwd").unwrap();
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
fn test_complex_nested_operations() {
    test_with_wasixcc("complex_nested_operations").unwrap();
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
fn test_active_context_id() {
    test_with_wasixcc("active_context_id").unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn test_wrong_entrypoint() {
    test_with_wasixcc("wrong_entrypoint").unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn test_switch_to_never_resumed() {
    test_with_wasixcc("switch_to_never_resumed").unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn test_function_args_preserved() {
    test_with_wasixcc("function_args_preserved").unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn test_correct_context_activated() {
    test_with_wasixcc("correct_context_activated").unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn test_switch_from_recursion() {
    test_with_wasixcc("switch_from_recursion").unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn test_global_ctx_ids() {
    test_with_wasixcc("global_ctx_ids").unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn test_shared_recursion() {
    test_with_wasixcc("shared_recursion").unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn test_mutual_recursion() {
    test_with_wasixcc("mutual_recursion").unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn test_three_way_recursion() {
    test_with_wasixcc("three_way_recursion").unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn test_context_exception_dynamic() {
    test_with_wasixcc("context_exception_dynamic").unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn test_context_exception_to_main() {
    test_with_wasixcc("context_exception_to_main").unwrap();
}