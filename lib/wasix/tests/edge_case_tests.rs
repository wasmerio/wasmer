#![cfg(all(unix, not(target_os = "macos"), not(feature = "js")))]
//! Edge case tests
//!
//! These tests verify various edge cases and corner cases in WASM/WASIX functionality,
//! including weak symbols, extern variables, and indirect function calls.

mod wasixcc_test_utils;

use wasixcc_test_utils::{run_build_script, run_wasm_with_result};

#[test]
fn test_weak_symbol_defined() {
    let wasm = run_build_script(file!(), "weak-symbol-defined").unwrap();
    let result = run_wasm_with_result(&wasm, wasm.parent().unwrap()).unwrap();
    let output = String::from_utf8_lossy(&result.stdout);
    assert_eq!(output.trim(), "other_func returned 42");
}

#[test]
fn test_weak_symbol_undefined() {
    let wasm = run_build_script(file!(), "weak-symbol-undefined").unwrap();
    let result = run_wasm_with_result(&wasm, wasm.parent().unwrap()).unwrap();
    let output = String::from_utf8_lossy(&result.stdout);
    assert_eq!(
        output.trim(),
        "other_func is not defined, but the program still compiled"
    );
}

#[test]
fn test_extern_variable() {
    let wasm = run_build_script(file!(), "extern-variable").unwrap();
    let result = run_wasm_with_result(&wasm, wasm.parent().unwrap()).unwrap();
    let output = String::from_utf8_lossy(&result.stdout);
    assert_eq!(output.trim(), "error number: 444");
}

#[test]
fn test_funky_problem() {
    let wasm = run_build_script(file!(), "funky-problem").unwrap();
    let result = run_wasm_with_result(&wasm, wasm.parent().unwrap()).unwrap();
    let output = String::from_utf8_lossy(&result.stdout);
    assert_eq!(output.trim(), ".Nothing weird happened");
}

#[test]
fn test_indirect_call_to_own_function_in_module() {
    let wasm = run_build_script(file!(), "indirect-call-to-own-function-in-module").unwrap();
    let result = run_wasm_with_result(&wasm, wasm.parent().unwrap()).unwrap();
    let output = String::from_utf8_lossy(&result.stdout);
    assert_eq!(output.trim(), "called");
}

#[test]
fn test_llvm_caching_problem() {
    let wasm = run_build_script(file!(), "llvm-caching-problem").unwrap();
    let result = run_wasm_with_result(&wasm, wasm.parent().unwrap()).unwrap();
    let output = String::from_utf8_lossy(&result.stdout);
    assert_eq!(output.trim(), "The dynamic library returned: 42");
}
