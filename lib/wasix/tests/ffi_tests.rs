#![cfg(all(unix, not(target_os = "macos"), not(feature = "js")))]
mod wasixcc_test_utils;

use wasixcc_test_utils::{run_build_script, run_wasm};

#[test]
fn simple_ffi_call() {
    let wasm = run_build_script(file!(), "simple-ffi-call").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn complex_ffi_call() {
    let wasm = run_build_script(file!(), "complex-ffi-call").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn longdouble_ffi_call() {
    let wasm = run_build_script(file!(), "longdouble-ffi-call").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn simple_ffi_closure() {
    let wasm = run_build_script(file!(), "simple-ffi-closure").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}
