#![cfg(all(unix, not(target_os = "macos"), not(feature = "js")))]
//! Exit tests from wasix-tests directory
//!
//! These tests verify various exit scenarios:
//! - exit-zero/exit-nonzero: Basic exit with status codes
//! - exit-*-in-thread: Exit from a pthread
//! - exit-*-in-fficall: Exit from an FFI callback
//! - exit-*-in-dyncall-thread: Exit from dynamically called thread
//! - exit-*-in-fficall-thread: Exit from FFI callback in thread

mod wasixcc_test_utils;

use wasixcc_test_utils::{run_build_script, run_wasm};

#[test]
fn test_exit_zero() {
    let wasm_path = run_build_script(file!(), "exit-zero").unwrap();
    let test_dir = wasm_path.parent().unwrap();
    run_wasm(&wasm_path, test_dir).unwrap();
}

#[test]
fn test_exit_nonzero() {
    let wasm_path = run_build_script(file!(), "exit-nonzero").unwrap();
    let test_dir = wasm_path.parent().unwrap();
    let result = run_wasm(&wasm_path, test_dir);
    assert!(
        result.is_err(),
        "exit-nonzero should fail with non-zero exit code"
    );
}

#[test]
fn test_exit_zero_in_thread() {
    let wasm_path = run_build_script(file!(), "exit-zero-in-thread").unwrap();
    let test_dir = wasm_path.parent().unwrap();
    run_wasm(&wasm_path, test_dir).unwrap();
}

#[test]
fn test_exit_nonzero_in_thread() {
    let wasm_path = run_build_script(file!(), "exit-nonzero-in-thread").unwrap();
    let test_dir = wasm_path.parent().unwrap();
    let result = run_wasm(&wasm_path, test_dir);
    // exit(99) in a thread should terminate the entire process with exit code 99
    assert!(
        result.is_err(),
        "exit-nonzero-in-thread should fail with non-zero exit code"
    );
}

#[test]
fn test_exit_zero_in_fficall() {
    let wasm_path = run_build_script(file!(), "exit-zero-in-fficall").unwrap();
    let test_dir = wasm_path.parent().unwrap();
    run_wasm(&wasm_path, test_dir).unwrap();
}

#[test]
fn test_exit_nonzero_in_fficall() {
    let wasm_path = run_build_script(file!(), "exit-nonzero-in-fficall").unwrap();
    let test_dir = wasm_path.parent().unwrap();
    let result = run_wasm(&wasm_path, test_dir);
    assert!(
        result.is_err(),
        "exit-nonzero-in-fficall should fail with non-zero exit code"
    );
}

#[test]
fn test_exit_zero_in_fficall_thread() {
    let wasm_path = run_build_script(file!(), "exit-zero-in-fficall-thread").unwrap();
    let test_dir = wasm_path.parent().unwrap();
    run_wasm(&wasm_path, test_dir).unwrap();
}

#[test]
fn test_exit_nonzero_in_fficall_thread() {
    let wasm_path = run_build_script(file!(), "exit-nonzero-in-fficall-thread").unwrap();
    let test_dir = wasm_path.parent().unwrap();
    let result = run_wasm(&wasm_path, test_dir);
    assert!(
        result.is_err(),
        "exit-nonzero-in-fficall-thread should fail with non-zero exit code"
    );
}

#[test]
fn test_exit_zero_in_dyncall_thread() {
    let wasm_path = run_build_script(file!(), "exit-zero-in-dyncall-thread").unwrap();
    let test_dir = wasm_path.parent().unwrap();
    run_wasm(&wasm_path, test_dir).unwrap();
}

#[test]
fn test_exit_nonzero_in_dyncall_thread() {
    let wasm_path = run_build_script(file!(), "exit-nonzero-in-dyncall-thread").unwrap();
    let test_dir = wasm_path.parent().unwrap();
    let result = run_wasm(&wasm_path, test_dir);
    assert!(
        result.is_err(),
        "exit-nonzero-in-dyncall-thread should fail with non-zero exit code"
    );
}
