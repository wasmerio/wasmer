use super::{run_build_script, run_wasm_with_result};

fn run_libc_test_stdout_0(test_name: &str) {
    let wasm = run_build_script(file!(), test_name).unwrap();
    let result = run_wasm_with_result(&wasm, wasm.parent().unwrap()).unwrap();
    let stdout = String::from_utf8_lossy(&result.stdout);
    let stderr = String::from_utf8_lossy(&result.stderr);
    let trace = String::from_utf8_lossy(&result.trace_output);
    assert_eq!(stdout.trim(), "0", "stderr:\n{}\ntrace:\n{}", stderr, trace);
    assert_eq!(
        result.exit_code,
        Some(0),
        "stdout:\n{}\nstderr:\n{}\ntrace:\n{}",
        stdout,
        stderr,
        trace,
    );
}

fn run_libc_test_stdout_0_in_temp_dir(test_name: &str) {
    let wasm = run_build_script(file!(), test_name).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let test_dir = temp.path();
    let result = run_wasm_with_result(&wasm, test_dir).unwrap();
    let stdout = String::from_utf8_lossy(&result.stdout);
    let stderr = String::from_utf8_lossy(&result.stderr);
    let trace = String::from_utf8_lossy(&result.trace_output);
    assert_eq!(stdout.trim(), "0", "stderr:\n{}\ntrace:\n{}", stderr, trace);
    assert_eq!(
        result.exit_code,
        Some(0),
        "stdout:\n{}\nstderr:\n{}\ntrace:\n{}",
        stdout,
        stderr,
        trace,
    );
}

wasm_test!(
    test_libc_clock_function,
    "libc-clock-function",
    stdout = "Clock works."
);
wasm_test!(
    test_libc_getpass,
    "libc-getpass",
    stdout = "getpass test - requires interactive terminal"
);
wasm_test!(
    test_mmap_anon,
    "mmap-anon",
    stdout = "mmap anon memory works"
);
wasm_test!(
    test_variadic_args,
    "variadic-args",
    stdout = "Printing 5, 6, 0, 42"
);

#[test]
#[ignore = "file-backed mmap writeback does not currently persist under WasiRunner"]
fn test_msync_end_of_file() {
    run_libc_test_stdout_0_in_temp_dir("msync-end-of-file");
}

#[test]
#[ignore = "file-backed mmap writeback does not currently persist under WasiRunner"]
fn test_msync_middle_of_file() {
    run_libc_test_stdout_0_in_temp_dir("msync-middle-of-file");
}

#[test]
#[ignore = "file-backed mmap writeback does not currently persist under WasiRunner"]
fn test_msync_start_of_file() {
    run_libc_test_stdout_0_in_temp_dir("msync-start-of-file");
}

#[test]
#[ignore = "file-backed mmap writeback does not currently persist under WasiRunner"]
fn test_munmap_sync_end_of_file() {
    run_libc_test_stdout_0_in_temp_dir("munmap-sync-end-of-file");
}

#[test]
#[ignore = "file-backed mmap writeback does not currently persist under WasiRunner"]
fn test_munmap_sync_middle_of_file() {
    run_libc_test_stdout_0_in_temp_dir("munmap-sync-middle-of-file");
}

#[test]
#[ignore = "file-backed mmap writeback does not currently persist under WasiRunner"]
fn test_munmap_sync_start_of_file() {
    run_libc_test_stdout_0_in_temp_dir("munmap-sync-start-of-file");
}

#[test]
#[ignore = "file-backed mmap writeback does not currently persist under WasiRunner"]
fn test_read_after_munmap() {
    run_libc_test_stdout_0_in_temp_dir("read-after-munmap");
}

#[test]
fn test_signal() {
    run_libc_test_stdout_0("signal");
}
