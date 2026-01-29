#![cfg(all(unix, not(target_os = "macos"), not(feature = "js")))]
//! Dynamic library tests
//!
//! These tests verify dynamic library loading and unloading functionality using dlopen/dlsym/dlclose.

mod wasixcc_test_utils;

use wasixcc_test_utils::{run_build_script, run_wasm_with_result};

#[test]
fn test_simple_dynamic_lib() {
    let wasm = run_build_script(file!(), "simple-dynamic-lib").unwrap();
    let result = run_wasm_with_result(&wasm, wasm.parent().unwrap()).unwrap();
    let output = String::from_utf8_lossy(&result.stdout);
    assert_eq!(output.trim(), "The shared library returned: 42");
}

#[test]
fn test_cpp_dynamic_lib() {
    let wasm = run_build_script(file!(), "cpp-dynamic-lib").unwrap();
    let result = run_wasm_with_result(&wasm, wasm.parent().unwrap()).unwrap();
    let output = String::from_utf8_lossy(&result.stdout);
    assert_eq!(output.trim(), "Hello world from C++");
}

#[test]
#[ignore = "Known failure - wasixcc fails dlclose-executes-destructors-in-the-correct-order"]
fn test_dlclose_executes_destructors() {
    let wasm = run_build_script(file!(), "dlclose-executes-destructors").unwrap();
    let result = run_wasm_with_result(&wasm, wasm.parent().unwrap()).unwrap();
    let output = String::from_utf8_lossy(&result.stdout);
    // Expected output:
    // a = main constructor
    // b = lib constructor
    // c = main code
    // d = lib destructor (on dlclose)
    // e = main code after dlclose
    // f = main destructor
    assert_eq!(output.trim(), "abcdef");
}

#[test]
fn test_duplicate_dynamic_lib() {
    let wasm = run_build_script(file!(), "duplicate-dynamic-lib").unwrap();
    let result = run_wasm_with_result(&wasm, wasm.parent().unwrap()).unwrap();
    let output = String::from_utf8_lossy(&result.stdout);

    let lines: Vec<&str> = output.trim().lines().collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0], "Module A returned: A");
    assert_eq!(lines[1], "Module B returned: B");
}

#[test]
#[ignore = "Currently broken - wasm-ld does not support recursive linking yet"]
fn test_recursive_shared_lib() {
    let wasm = run_build_script(file!(), "recursive-shared-lib").unwrap();
    let result = run_wasm_with_result(&wasm, wasm.parent().unwrap()).unwrap();
    let output = String::from_utf8_lossy(&result.stdout);
    assert_eq!(output.trim(), "The shared library returned: 42");
}
