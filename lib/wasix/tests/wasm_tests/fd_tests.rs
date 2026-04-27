use super::{run_build_script, run_wasm};

#[test]
fn test_fd_allocate() {
    let wasm = run_build_script(file!(), "fd-allocate").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn test_fd_open_readonly() {
    let wasm = run_build_script(file!(), "fd-open-readonly").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}
