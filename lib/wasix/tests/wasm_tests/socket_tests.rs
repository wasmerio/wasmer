use super::{run_build_script, run_wasm, run_wasm_with_result};

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

#[test]
// https://github.com/wasmerio/wasmer/issues/6403
fn test_bind_port_zero_allocates_ephemeral_port() {
    let wasm = run_build_script(file!(), "bind-port-zero").unwrap();
    let result = run_wasm_with_result(&wasm, wasm.parent().unwrap()).unwrap();
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert_eq!(
        stdout.trim(),
        "bind port 0 allocates an ephemeral port",
        "exit_code={:?}\nstdout:\n{}\nstderr:\n{}",
        result.exit_code,
        stdout,
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
// https://github.com/wasmerio/wasmer/issues/6403
fn test_bind_port_zero_keeps_same_port_across_connect() {
    let wasm = run_build_script(file!(), "bind-port-zero-connect").unwrap();
    let result = run_wasm_with_result(&wasm, wasm.parent().unwrap()).unwrap();
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert_eq!(
        stdout.trim(),
        "bind port 0 keeps the same ephemeral port across connect",
        "exit_code={:?}\nstdout:\n{}\nstderr:\n{}",
        result.exit_code,
        stdout,
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn test_bind_fail_leaves_socket_unbound() {
    let wasm = run_build_script(file!(), "bind-fail-stays-unbound").unwrap();
    let result = run_wasm_with_result(&wasm, wasm.parent().unwrap()).unwrap();
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert_eq!(
        stdout.trim(),
        "bind failure leaves socket unbound",
        "exit_code={:?}\nstdout:\n{}\nstderr:\n{}",
        result.exit_code,
        stdout,
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
// https://github.com/wasmerio/wasmer/issues/6366
#[ignore = "flaky test (#6366)"]
fn test_socket_pair() {
    let wasm = run_build_script(file!(), "socket-pair").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}
