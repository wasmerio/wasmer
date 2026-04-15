use super::{run_build_script, run_wasm};

#[test]
fn call_dynamic_strict_and_nonstrict() {
    let wasm = run_build_script(file!(), "call-dynamic-strict-and-nonstrict").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn call_dynamic_complex_abi() {
    let wasm = run_build_script(file!(), "call-dynamic-complex-abi").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn call_dynamic_buffer_modes() {
    let wasm = run_build_script(file!(), "call-dynamic-buffer-modes").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn direct_closure_prepare() {
    let wasm = run_build_script(file!(), "closure-lifecycle").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}
