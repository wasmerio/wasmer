use super::{run_build_script, run_wasm};

#[test]
fn fd_fdstat_set_rights() {
    let wasm = run_build_script(file!(), "").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}
