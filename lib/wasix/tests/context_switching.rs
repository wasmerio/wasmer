#![cfg(all(unix, not(target_os = "macos"), not(feature = "js")))]
mod wasixcc_test_utils;

use wasixcc_test_utils::{run_build_script, run_wasm};

// macOS is currently disabled, because cranelift does not
// support exception handling on that platform yet.
#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn test_simple_switching() {
    let wasm_path = run_build_script(file!(), "simple_switching").unwrap();
    let test_dir = wasm_path.parent().unwrap();
    run_wasm(&wasm_path, test_dir).unwrap();
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn test_switching_with_main() {
    let wasm_path = run_build_script(file!(), "switching_with_main").unwrap();
    let test_dir = wasm_path.parent().unwrap();
    run_wasm(&wasm_path, test_dir).unwrap();
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn test_switching_to_a_deleted_context() {
    let wasm_path = run_build_script(file!(), "switching_to_a_deleted_context").unwrap();
    let test_dir = wasm_path.parent().unwrap();
    run_wasm(&wasm_path, test_dir).unwrap();
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn test_switching_threads() {
    let wasm_path = run_build_script(file!(), "switching_in_threads").unwrap();
    let test_dir = wasm_path.parent().unwrap();
    run_wasm(&wasm_path, test_dir).unwrap();
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn test_multiple_contexts() {
    let wasm_path = run_build_script(file!(), "multiple_contexts").unwrap();
    let test_dir = wasm_path.parent().unwrap();
    run_wasm(&wasm_path, test_dir).unwrap();
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn test_error_handling() {
    let wasm_path = run_build_script(file!(), "error_handling").unwrap();
    let test_dir = wasm_path.parent().unwrap();
    run_wasm(&wasm_path, test_dir).unwrap();
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn test_nested_switches() {
    let wasm_path = run_build_script(file!(), "nested_switches").unwrap();
    let test_dir = wasm_path.parent().unwrap();
    run_wasm(&wasm_path, test_dir).unwrap();
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn test_contexts_with_mutexes() {
    let wasm_path = run_build_script(file!(), "contexts_with_mutexes").unwrap();
    let test_dir = wasm_path.parent().unwrap();
    run_wasm(&wasm_path, test_dir).unwrap();
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn test_contexts_with_env_vars() {
    let wasm_path = run_build_script(file!(), "contexts_with_env_vars").unwrap();
    let test_dir = wasm_path.parent().unwrap();
    run_wasm(&wasm_path, test_dir).unwrap();
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn test_contexts_with_signals() {
    let wasm_path = run_build_script(file!(), "contexts_with_signals").unwrap();
    let test_dir = wasm_path.parent().unwrap();
    run_wasm(&wasm_path, test_dir).unwrap();
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn test_contexts_with_timers() {
    let wasm_path = run_build_script(file!(), "contexts_with_timers").unwrap();
    let test_dir = wasm_path.parent().unwrap();
    run_wasm(&wasm_path, test_dir).unwrap();
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn test_contexts_with_pipes() {
    let wasm_path = run_build_script(file!(), "contexts_with_pipes").unwrap();
    let test_dir = wasm_path.parent().unwrap();
    run_wasm(&wasm_path, test_dir).unwrap();
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn test_pending_file_operations() {
    let wasm_path = run_build_script(file!(), "pending_file_operations").unwrap();
    let test_dir = wasm_path.parent().unwrap();
    run_wasm(&wasm_path, test_dir).unwrap();
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn test_recursive_host_calls() {
    let wasm_path = run_build_script(file!(), "recursive_host_calls").unwrap();
    let test_dir = wasm_path.parent().unwrap();
    run_wasm(&wasm_path, test_dir).unwrap();
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn test_malloc_during_switch() {
    let wasm_path = run_build_script(file!(), "malloc_during_switch").unwrap();
    let test_dir = wasm_path.parent().unwrap();
    run_wasm(&wasm_path, test_dir).unwrap();
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn test_nested_host_call_switch() {
    let wasm_path = run_build_script(file!(), "nested_host_call_switch").unwrap();
    let test_dir = wasm_path.parent().unwrap();
    run_wasm(&wasm_path, test_dir).unwrap();
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn test_switch_to_never_resumed() {
    let wasm_path = run_build_script(file!(), "switch_to_never_resumed").unwrap();
    let test_dir = wasm_path.parent().unwrap();
    run_wasm(&wasm_path, test_dir).unwrap();
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn test_three_way_recursion() {
    let wasm_path = run_build_script(file!(), "three_way_recursion").unwrap();
    let test_dir = wasm_path.parent().unwrap();
    run_wasm(&wasm_path, test_dir).unwrap();
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn test_contexts_with_setjmp() {
    let wasm_path = run_build_script(file!(), "contexts_with_setjmp").unwrap();
    let test_dir = wasm_path.parent().unwrap();
    run_wasm(&wasm_path, test_dir).unwrap();
}
