mod wasixcc_test_utils;
use wasixcc_test_utils::{run_build_script, run_wasm};

#[test]
fn wasix_reflection() {
    let wasm = run_build_script(file!(), "wasix-reflection").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn wasix_reflection_and_closures() {
    let wasm = run_build_script(file!(), "wasix-reflection-and-closures").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn wasix_reflection_dlopen() {
    let wasm = run_build_script(file!(), "wasix-reflection-dlopen").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn wasix_reflection_static() {
    let wasm = run_build_script(file!(), "wasix-reflection-static").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}
