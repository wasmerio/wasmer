use super::{run_build_script, run_wasm};

#[test]
fn test_thread_spawn() {
    let wasm = run_build_script(file!(), ".").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}
