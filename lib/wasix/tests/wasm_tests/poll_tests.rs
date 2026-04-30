use super::{run_build_script, run_wasm, run_wasm_with_result};

#[test]
fn test_epoll_create() {
    let wasm = run_build_script(file!(), "epoll-create").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn test_poll_oneoff_zero() {
    let wasm = run_build_script(file!(), "poll-oneoff-zero").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn test_epoll_create_ctl_wait() {
    let wasm = run_build_script(file!(), "epoll-create-ctl-wait").unwrap();
    let result = run_wasm_with_result(&wasm, wasm.parent().unwrap()).unwrap();
    assert_eq!(String::from_utf8_lossy(&result.stdout).trim(), "0");
    assert_eq!(result.exit_code, Some(0));
}

#[test]
fn test_poll_oneoff() {
    let wasm = run_build_script(file!(), "poll-oneoff").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}
