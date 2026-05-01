use super::{run_build_script, run_wasm_with_result};

wasm_test!(test_epoll_create, "epoll-create");
wasm_test!(test_poll_oneoff_zero, "poll-oneoff-zero");
wasm_test!(test_poll_oneoff, "poll-oneoff");
wasm_test!(test_eventfd_semaphore_read, "eventfd-semaphore-read");

// Checks both stdout and exit_code explicitly.
#[test]
fn test_epoll_create_ctl_wait() {
    let wasm = run_build_script(file!(), "epoll-create-ctl-wait").unwrap();
    let result = run_wasm_with_result(&wasm, wasm.parent().unwrap()).unwrap();
    assert_eq!(String::from_utf8_lossy(&result.stdout).trim(), "0");
    assert_eq!(result.exit_code, Some(0));
}
