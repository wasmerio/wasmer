use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;

use super::{run_build_script, run_wasm_with_result};

#[test]
fn test_lock_timeout() {
    let wasm = run_build_script(file!(), "mutex-lock-timeout").unwrap();
    let dir = wasm.parent().unwrap().to_path_buf();

    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(run_wasm_with_result(&wasm, &dir));
    });

    let result = match receiver.recv_timeout(Duration::from_secs(5)) {
        Ok(result) => result.unwrap(),
        Err(RecvTimeoutError::Timeout) => panic!("mutex-lock-timeout timed out after 5 seconds"),
        Err(RecvTimeoutError::Disconnected) => panic!("mutex-lock-timeout runner thread exited early"),
    };

    assert_eq!(
        result.exit_code,
        Some(0),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}
