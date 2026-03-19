use super::{run_build_script, run_wasm_with_result};
use wasmer::Engine;
use wasmer::sys::{Features, LLVM, NativeEngineExt, Target};

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
