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
fn test_socket_pair() {
    let wasm = run_build_script(file!(), "socket-pair").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}
