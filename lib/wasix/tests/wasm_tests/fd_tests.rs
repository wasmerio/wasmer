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

fn run_fd_test(test_name: &str, cleanup_paths: &[&str]) {
    let wasm = run_build_script(file!(), test_name).unwrap();
    let test_dir = wasm.parent().unwrap();
    for path in cleanup_paths {
        remove_path_if_exists(&test_dir.join(path));
    }

    let result = run_wasm_with_result(&wasm, test_dir).unwrap();

    for path in cleanup_paths {
        remove_path_if_exists(&test_dir.join(path));
    }
    assert_eq!(result.exit_code, Some(0));
}

wasm_test!(test_fd_allocate, "fd-allocate");
wasm_test!(test_fd_open_readonly, "fd-open-readonly");

wasm_test!(test_fd_close, "fd-close");

#[test]
fn test_pipes() {
    run_fd_test("pipes", &[]);
}

#[test]
fn test_pwrite_and_size() {
    let wasm = run_build_script(file!(), "pwrite-and-size").unwrap();
    let test_dir = wasm.parent().unwrap();
    remove_path_if_exists(&test_dir.join("my_file.txt"));

    let result = run_wasm_with_result(&wasm, test_dir).unwrap();

    remove_path_if_exists(&test_dir.join("my_file.txt"));
    assert_eq!(String::from_utf8_lossy(&result.stdout).trim(), "0");
    assert_eq!(result.exit_code, Some(0));
}
