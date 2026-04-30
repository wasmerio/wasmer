//! Exit tests from wasix-tests directory
//!
//! These tests verify various exit scenarios:
//! - exit-zero/exit-nonzero: Basic exit with status codes
//! - exit-*-in-thread: Exit from a pthread
//! - exit-*-in-closure-call: Exit from a closure callback
//! - exit-*-in-call-dynamic-thread: Exit from dynamically called thread
//! - exit-*-in-closure-call-thread: Exit from a closure callback in thread

use super::{run_build_script, run_wasm};

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
fn test_exit_zero_in_closure_call() {
    let wasm_path = run_build_script(file!(), "exit-zero-in-closure-call").unwrap();
    let test_dir = wasm_path.parent().unwrap();
    run_wasm(&wasm_path, test_dir).unwrap();
}

#[test]
fn test_exit_nonzero_in_closure_call() {
    let wasm_path = run_build_script(file!(), "exit-nonzero-in-closure-call").unwrap();
    let test_dir = wasm_path.parent().unwrap();
    let result = run_wasm(&wasm_path, test_dir);
    assert!(
        result.is_err(),
        "exit-nonzero-in-closure-call should fail with non-zero exit code"
    );
}

#[test]
fn test_exit_zero_in_closure_call_thread() {
    let wasm_path = run_build_script(file!(), "exit-zero-in-closure-call-thread").unwrap();
    let test_dir = wasm_path.parent().unwrap();
    run_wasm(&wasm_path, test_dir).unwrap();
}

#[test]
fn test_exit_nonzero_in_closure_call_thread() {
    let wasm_path = run_build_script(file!(), "exit-nonzero-in-closure-call-thread").unwrap();
    let test_dir = wasm_path.parent().unwrap();
    let result = run_wasm(&wasm_path, test_dir);
    assert!(
        result.is_err(),
        "exit-nonzero-in-closure-call-thread should fail with non-zero exit code"
    );
}

#[test]
fn test_exit_zero_in_call_dynamic_thread() {
    let wasm_path = run_build_script(file!(), "exit-zero-in-call-dynamic-thread").unwrap();
    let test_dir = wasm_path.parent().unwrap();
    run_wasm(&wasm_path, test_dir).unwrap();
}

#[test]
fn test_exit_nonzero_in_call_dynamic_thread() {
    let wasm_path = run_build_script(file!(), "exit-nonzero-in-call-dynamic-thread").unwrap();
    let test_dir = wasm_path.parent().unwrap();
    let result = run_wasm(&wasm_path, test_dir);
    assert!(
        result.is_err(),
        "exit-nonzero-in-call-dynamic-thread should fail with non-zero exit code"
    );
}
