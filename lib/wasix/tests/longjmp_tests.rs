#![cfg(all(unix, not(target_os = "macos"), not(feature = "js")))]
//! Longjmp tests
//!
//! These tests verify setjmp/longjmp functionality within and across module boundaries.

mod wasixcc_test_utils;

use wasixcc_test_utils::{run_build_script, run_wasm_with_result};

#[test]
fn test_simple_longjmp() {
    let wasm = run_build_script(file!(), "simple-longjmp").unwrap();
    let result = run_wasm_with_result(&wasm, wasm.parent().unwrap()).unwrap();
    let output = String::from_utf8_lossy(&result.stdout);
    assert_eq!(output.trim(), "abc");
}

#[test]
fn test_longjmp_in_library() {
    let wasm = run_build_script(file!(), "longjmp-in-library").unwrap();
    let result = run_wasm_with_result(&wasm, wasm.parent().unwrap()).unwrap();
    let output = String::from_utf8_lossy(&result.stdout);
    assert_eq!(output.trim(), "abc");
}

#[test]
fn test_longjmp_across_libraries() {
    let wasm = run_build_script(file!(), "longjmp-across-libraries").unwrap();
    let result = run_wasm_with_result(&wasm, wasm.parent().unwrap()).unwrap();
    let output = String::from_utf8_lossy(&result.stdout);
    assert_eq!(output.trim(), "abc");
}

#[test]
fn test_longjmp_across_libraries_buffer_in_lib() {
    let wasm = run_build_script(file!(), "longjmp-across-libraries-buffer-in-lib").unwrap();
    let result = run_wasm_with_result(&wasm, wasm.parent().unwrap()).unwrap();
    let output = String::from_utf8_lossy(&result.stdout);
    assert_eq!(output.trim(), "abc");
}
