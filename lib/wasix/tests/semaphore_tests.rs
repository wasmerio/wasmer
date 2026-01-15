//! Semaphore tests
//!
//! These tests verify POSIX semaphore functionality:
//! - Named semaphores (sem_open, sem_close, sem_unlink)
//! - Unnamed semaphores (sem_init, sem_destroy)
//! - Semaphore operations (sem_wait, sem_post)
//! - Edge cases and error handling

mod wasixcc_test_utils;
use wasixcc_test_utils::{run_build_script, run_wasm, run_wasm_with_result};

#[test]
fn test_semaphore_named() {
    let wasm_path = run_build_script(file!(), "semaphore-named").unwrap();
    let test_dir = wasm_path.parent().unwrap();
    run_wasm(&wasm_path, test_dir).unwrap();
}

#[test]
fn test_semaphore_unnamed() {
    let wasm_path = run_build_script(file!(), "semaphore-unnamed").unwrap();
    let test_dir = wasm_path.parent().unwrap();
    run_wasm(&wasm_path, test_dir).unwrap();
}

#[test]
fn test_semaphore_open_without_create() {
    let wasm_path = run_build_script(file!(), "semaphore-open-without-create").unwrap();
    let test_dir = wasm_path.parent().unwrap();
    run_wasm(&wasm_path, test_dir).unwrap();
}

#[test]
fn test_semaphore_open_invalid_names() {
    let wasm_path = run_build_script(file!(), "semaphore-open-invalid-names").unwrap();
    let test_dir = wasm_path.parent().unwrap();
    run_wasm(&wasm_path, test_dir).unwrap();
}

#[test]
fn test_semaphore_same_name_no_create_on_second() {
    let wasm_path = run_build_script(file!(), "semaphore-same-name-no-create-on-second").unwrap();
    let test_dir = wasm_path.parent().unwrap();
    run_wasm(&wasm_path, test_dir).unwrap();
}

#[test]
fn test_semaphore_same_name_twice_with_excl() {
    let wasm_path = run_build_script(file!(), "semaphore-same-name-twice-with-excl").unwrap();
    let test_dir = wasm_path.parent().unwrap();
    run_wasm(&wasm_path, test_dir).unwrap();
}

#[test]
fn test_semaphore_same_name_twice_without_excl() {
    let wasm_path = run_build_script(file!(), "semaphore-same-name-twice-without-excl").unwrap();
    let test_dir = wasm_path.parent().unwrap();
    run_wasm(&wasm_path, test_dir).unwrap();
}

#[test]
fn test_semaphore_unlink_named() {
    let wasm_path = run_build_script(file!(), "semaphore-unlink-named").unwrap();
    let test_dir = wasm_path.parent().unwrap();
    run_wasm(&wasm_path, test_dir).unwrap();
}

#[test]
fn test_semaphore_unlink_nonexistent() {
    let wasm_path = run_build_script(file!(), "semaphore-unlink-nonexistent").unwrap();
    let test_dir = wasm_path.parent().unwrap();
    run_wasm(&wasm_path, test_dir).unwrap();
}

#[test]
fn test_semaphore_unlink_nullptr_exits() {
    let wasm_path = run_build_script(file!(), "semaphore-unlink-nullptr-exits").unwrap();
    let test_dir = wasm_path.parent().unwrap();
    let result = run_wasm_with_result(&wasm_path, test_dir).unwrap();

    // The test should exit with a non-zero code (assertion failure)
    assert!(result.exit_code.is_some(), "Expected an exit code");
    assert_ne!(result.exit_code.unwrap(), 0, "Expected non-zero exit code");
}
