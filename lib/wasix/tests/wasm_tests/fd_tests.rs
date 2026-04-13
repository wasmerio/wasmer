use super::{run_build_script, run_wasm};

#[test]
fn test_fd_allocate() {
    let wasm = run_build_script(file!(), "fd-allocate").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn test_unlink_open_fd_write_after_unlink() {
    let wasm = run_build_script(file!(), "unlink-open-fd-write-after-unlink").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}
