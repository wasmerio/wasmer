use super::{run_build_script, run_wasm, run_wasm_with_stdin};
use wasmer_wasix::Pipe;

#[test]
fn test_fd_allocate() {
    let wasm = run_build_script(file!(), "fd-allocate").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

/// Regression test for fd_read blocking DL operations.
///
/// One WASM thread blocks inside `fd_read` on stdin (which never produces
/// data). While that thread is parked, the main WASM thread must be able to
/// call `dlopen` and load a shared library without deadlocking.  Before the
/// fix, `fd_read` held a lock that prevented DL operations from proceeding.
#[test]
fn test_stdin_read_does_not_block_dlopen() {
    let wasm = run_build_script(file!(), "stdin-dlopen-race").unwrap();

    // Keep the write end alive so stdin remains blocked for the whole guest
    // run; the guest exits explicitly after proving dlopen works.
    let (_pipe_tx, pipe_rx) = Pipe::channel();

    let result = run_wasm_with_stdin(&wasm, wasm.parent().unwrap(), Box::new(pipe_rx)).unwrap();

    let stdout = String::from_utf8_lossy(&result.stdout);
    assert_eq!(
        stdout.trim(),
        "reader_ready\ndlopen_succeeded_after_reader_ready\nside_value=42\nsequence_ok",
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(result.exit_code, Some(0));
}

#[test]
fn test_stdin_read_is_interrupted_by_signal() {
    let wasm = run_build_script(file!(), "stdin-signal-eintr").unwrap();

    // Keep stdin blocked for the duration of the test; the guest reader thread
    // should wake because of a signal and return EINTR rather than waiting for
    // input or EOF.
    let (_pipe_tx, pipe_rx) = Pipe::channel();

    let result = run_wasm_with_stdin(&wasm, wasm.parent().unwrap(), Box::new(pipe_rx)).unwrap();

    let stdout = String::from_utf8_lossy(&result.stdout);
    assert_eq!(
        stdout.trim(),
        "reader_ready\nsignal_sent\nhandler_called\nread_errno=EINTR\nsequence_ok",
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(result.exit_code, Some(0));
}

#[test]
fn test_fd_open_readonly() {
    let wasm = run_build_script(file!(), "fd-open-readonly").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}
