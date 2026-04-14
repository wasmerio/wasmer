use super::{run_build_script, run_wasm};

#[test]
fn call_dynamic_strict_and_nonstrict() {
    let wasm = run_build_script(file!(), "simple-ffi-call").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn call_dynamic_complex_abi() {
    let wasm = run_build_script(file!(), "complex-ffi-call").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn call_dynamic_buffer_modes() {
    let wasm = run_build_script(file!(), "longdouble-ffi-call").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn direct_closure_prepare() {
    let wasm = run_build_script(file!(), "simple-ffi-closure").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}
