#![cfg(all(unix, not(target_os = "macos"), not(feature = "js")))]
mod wasixcc_test_utils;

use wasixcc_test_utils::{run_build_script, run_wasm};

#[test]
fn simple_exceptions() {
    let wasm = run_build_script(file!(), "simple-exceptions").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn simple_exceptions_with_lto() {
    let wasm = run_build_script(file!(), "simple-exceptions-with-lto").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn simple_exceptions_with_shared_lib_in_callstack() {
    let wasm = run_build_script(file!(), "simple-exceptions-with-shared-lib-in-callstack").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn simple_exceptions_with_shared_lib_present() {
    let wasm = run_build_script(file!(), "simple-exceptions-with-shared-lib-present").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn exceptions_catchall_in_shared_lib() {
    let wasm = run_build_script(file!(), "exceptions-catchall-in-shared-lib").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn exceptions_in_main_and_shared() {
    let wasm = run_build_script(file!(), "exceptions-in-main-and-shared").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

// Grid tests for exceptions-across-modules with different combinations
// of static/shared linking for thrower and catcher modules

#[test]
fn static_thrower_static_catcher() {
    let wasm = run_build_script(file!(), "static-thrower-static-catcher").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn static_thrower_shared_catcher() {
    let wasm = run_build_script(file!(), "static-thrower-shared-catcher").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn shared_thrower_static_catcher() {
    let wasm = run_build_script(file!(), "shared-thrower-static-catcher").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn shared_thrower_shared_catcher() {
    let wasm = run_build_script(file!(), "shared-thrower-shared-catcher").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn static_thrower_via_shared_proxy_static_catcher() {
    let wasm = run_build_script(file!(), "static-thrower-via-shared-proxy-static-catcher").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}
