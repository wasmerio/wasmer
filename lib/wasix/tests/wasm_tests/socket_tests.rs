use super::{run_build_script, run_wasm_with_result};

// These tests include stderr in the assertion message for easier debugging.
#[test]
fn test_pipe_send_recv_compat() {
    let wasm = run_build_script(file!(), "pipe_send_recv_compat").unwrap();
    let result = run_wasm_with_result(&wasm, wasm.parent().unwrap()).unwrap();
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert_eq!(
        stdout.trim(),
        "pipe send/recv works",
        "exit_code={:?}\nstdout:\n{}\nstderr:\n{}",
        result.exit_code,
        stdout,
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn test_nonblocking_connect() {
    let wasm = run_build_script(file!(), "nonblocking-connect").unwrap();
    let result = run_wasm_with_result(&wasm, wasm.parent().unwrap()).unwrap();
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert_eq!(
        stdout.trim(),
        "nonblocking connect returned immediately",
        "exit_code={:?}\nstdout:\n{}\nstderr:\n{}",
        result.exit_code,
        stdout,
        String::from_utf8_lossy(&result.stderr)
    );
}

// https://github.com/wasmerio/wasmer/issues/6366
wasm_test!(
    #[ignore = "flaky test (#6366)"]
    test_socket_pair,
    "socket-pair"
);
