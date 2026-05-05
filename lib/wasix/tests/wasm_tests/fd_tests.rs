use super::{run_build_script, run_wasm_with_result};

wasm_test!(test_fd_allocate, "fd-allocate");
wasm_test!(test_fd_open_readonly, "fd-open-readonly");

wasm_test!(test_fd_close, "fd-close");

#[test]
fn test_pipes() {
    let wasm = run_build_script(file!(), "pipes").unwrap();
    let test_dir = wasm.parent().unwrap();
    let result = run_wasm_with_result(&wasm, test_dir).unwrap();
    assert_eq!(result.exit_code, Some(0));
}

#[test]
fn test_pwrite_and_size() {
    let wasm = run_build_script(file!(), "pwrite-and-size").unwrap();
    let temp = tempfile::tempdir().unwrap();
    let result = run_wasm_with_result(&wasm, temp.path()).unwrap();
    assert_eq!(String::from_utf8_lossy(&result.stdout).trim(), "0");
    assert_eq!(result.exit_code, Some(0));
}
