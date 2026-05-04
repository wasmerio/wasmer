use std::fs;
use std::path::Path;

use super::{run_build_script, run_wasm_with_result};

fn remove_path_if_exists(path: &Path) {
    if !path.exists() {
        return;
    }

    if path.is_dir() {
        let _ = fs::remove_dir_all(path);
    } else {
        let _ = fs::remove_file(path);
    }
}

fn run_libc_test_stdout_0(test_name: &str, cleanup_paths: &[&str]) {
    let wasm = run_build_script(file!(), test_name).unwrap();
    let test_dir = wasm.parent().unwrap();
    for path in cleanup_paths {
        remove_path_if_exists(&test_dir.join(path));
    }

    let result = run_wasm_with_result(&wasm, test_dir).unwrap();

    for path in cleanup_paths {
        remove_path_if_exists(&test_dir.join(path));
    }
    assert_eq!(String::from_utf8_lossy(&result.stdout).trim(), "0");
    assert_eq!(result.exit_code, Some(0));
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
    run_libc_test_stdout_0("msync-end-of-file", &["my_file.txt"]);
}

#[test]
#[ignore = "file-backed mmap writeback does not currently persist under WasiRunner"]
fn test_msync_middle_of_file() {
    run_libc_test_stdout_0("msync-middle-of-file", &["my_file.txt"]);
}

#[test]
#[ignore = "file-backed mmap writeback does not currently persist under WasiRunner"]
fn test_msync_start_of_file() {
    run_libc_test_stdout_0("msync-start-of-file", &["my_file.txt"]);
}

#[test]
#[ignore = "file-backed mmap writeback does not currently persist under WasiRunner"]
fn test_munmap_sync_end_of_file() {
    run_libc_test_stdout_0("munmap-sync-end-of-file", &["my_file.txt"]);
}

#[test]
#[ignore = "file-backed mmap writeback does not currently persist under WasiRunner"]
fn test_munmap_sync_middle_of_file() {
    run_libc_test_stdout_0("munmap-sync-middle-of-file", &["my_file.txt"]);
}

#[test]
#[ignore = "file-backed mmap writeback does not currently persist under WasiRunner"]
fn test_munmap_sync_start_of_file() {
    run_libc_test_stdout_0("munmap-sync-start-of-file", &["my_file.txt"]);
}

#[test]
#[ignore = "file-backed mmap writeback does not currently persist under WasiRunner"]
fn test_read_after_munmap() {
    run_libc_test_stdout_0("read-after-munmap", &["my_file.txt"]);
}

#[test]
fn test_signal() {
    run_libc_test_stdout_0("signal", &[]);
}
