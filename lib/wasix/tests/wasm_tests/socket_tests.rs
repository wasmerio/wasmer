use super::{run_build_script, run_wasm};

#[test]
fn test_socket_pair() {
    let wasm = run_build_script(file!(), "socket-pair").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}
