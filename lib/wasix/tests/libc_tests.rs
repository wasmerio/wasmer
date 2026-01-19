//! Libc function tests
//!
//! These tests verify various libc functions work correctly in WASIX.

mod wasixcc_test_utils;

use wasixcc_test_utils::{run_build_script, run_wasm_with_result};

#[test]
fn test_libc_clock_function() {
    let wasm = run_build_script(file!(), "libc-clock-function").unwrap();
    let result = run_wasm_with_result(&wasm, wasm.parent().unwrap()).unwrap();
    let output = String::from_utf8_lossy(&result.stdout);
    assert_eq!(output.trim(), "Clock works.");
}

#[test]
fn test_libc_getpass() {
    let wasm = run_build_script(file!(), "libc-getpass").unwrap();
    let result = run_wasm_with_result(&wasm, wasm.parent().unwrap()).unwrap();
    let output = String::from_utf8_lossy(&result.stdout);
    assert_eq!(
        output.trim(),
        "getpass test - requires interactive terminal"
    );
}

#[test]
fn test_mmap_anon() {
    let wasm = run_build_script(file!(), "mmap-anon").unwrap();
    let result = run_wasm_with_result(&wasm, wasm.parent().unwrap()).unwrap();
    let output = String::from_utf8_lossy(&result.stdout);
    assert_eq!(output.trim(), "mmap anon memory works");
}

#[test]
fn test_variadic_args() {
    let wasm = run_build_script(file!(), "variadic-args").unwrap();
    let result = run_wasm_with_result(&wasm, wasm.parent().unwrap()).unwrap();
    let output = String::from_utf8_lossy(&result.stdout);
    assert_eq!(output.trim(), "Printing 5, 6, 0, 42");
}
