use super::{run_build_script, run_wasm};

#[test]
fn context_destroy() {
    let wasm = run_build_script(file!(), "").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}
