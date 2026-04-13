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

#[test]
fn test_chdir_getcwd() {
    let wasm = run_build_script(file!(), "chdir-getcwd").unwrap();
    let result = run_wasm_with_result(&wasm, wasm.parent().unwrap()).unwrap();
    assert_eq!(String::from_utf8_lossy(&result.stdout).trim(), "0");
    assert_eq!(result.exit_code, Some(0));
}

#[test]
fn test_create_move_open() {
    let wasm = run_build_script(file!(), "create-move-open").unwrap();
    let test_dir = wasm.parent().unwrap();
    remove_path_if_exists(&test_dir.join("test1"));
    remove_path_if_exists(&test_dir.join("test2"));
    let result = run_wasm_with_result(&wasm, test_dir).unwrap();
    remove_path_if_exists(&test_dir.join("test1"));
    remove_path_if_exists(&test_dir.join("test2"));
    assert_eq!(String::from_utf8_lossy(&result.stdout).trim(), "0");
    assert_eq!(result.exit_code, Some(0));
}
