#![cfg(all(unix, not(target_os = "macos"), not(feature = "js")))]
//! Shared library tests from wasix-tests directory
//!
//! These tests verify that shared libraries (.so) work correctly:
//! - simple-shared-lib: Basic shared library with a function
//! - errno-in-shared-lib: errno (thread-local) works correctly in shared libraries
//! - simple-exceptions-in-shared-lib: C++ exceptions in shared libraries

mod wasixcc_test_utils;

use wasixcc_test_utils::{run_build_script, run_wasm};

#[test]
fn test_simple_shared_lib() {
    let wasm_path = run_build_script(file!(), "simple-shared-lib").unwrap();
    let test_dir = wasm_path.parent().unwrap();
    run_wasm(&wasm_path, test_dir).unwrap();
}

#[test]
fn test_errno_in_shared_lib() {
    let wasm_path = run_build_script(file!(), "errno-in-shared-lib").unwrap();
    let test_dir = wasm_path.parent().unwrap();
    run_wasm(&wasm_path, test_dir).unwrap();
}

#[test]
fn test_simple_exceptions_in_shared_lib() {
    let wasm_path = run_build_script(file!(), "simple-exceptions-in-shared-lib").unwrap();
    let test_dir = wasm_path.parent().unwrap();
    run_wasm(&wasm_path, test_dir).unwrap();
}
