use std::fs;
use std::path::Path;

use super::{
    MappedDirectory, run_build_script, run_wasm_with_result, run_wasm_with_runner_config_checked,
};

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

fn run_path_test_stdout_0(test_name: &str, cleanup_paths: &[&str]) {
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

#[test]
fn test_chdir_getcwd() {
    let wasm = run_build_script(file!(), "chdir-getcwd").unwrap();
    let result = run_wasm_with_result(&wasm, wasm.parent().unwrap()).unwrap();
    assert_eq!(String::from_utf8_lossy(&result.stdout).trim(), "0");
    assert_eq!(result.exit_code, Some(0));
}

#[test]
fn test_closing_pre_opened_dirs() {
    run_path_test_stdout_0("closing-pre-opened-dirs", &[]);
}

#[test]
fn test_create_and_remove_dirs() {
    run_path_test_stdout_0("create-and-remove-dirs", &["test1"]);
}

#[test]
fn test_create_dir_at_cwd() {
    run_path_test_stdout_0("create-dir-at-cwd", &["test1", "test2", "test3", "test4"]);
}

#[test]
fn test_create_dir_at_cwd_with_chdir() {
    run_path_test_stdout_0(
        "create-dir-at-cwd-with-chdir",
        &["test1", "test2", "test3", "test4"],
    );
}

#[test]
fn test_cwd_to_home() {
    run_path_test_stdout_0("cwd-to-home", &[]);
}

#[test]
fn test_distinct_inodes_same_basename() {
    run_path_test_stdout_0("distinct-inodes-same-basename", &["src", "dst"]);
}

#[test]
fn test_fstatat_with_chdir() {
    run_path_test_stdout_0("fstatat-with-chdir", &["test1", "test2"]);
}

#[test]
fn test_mount_tmp_locally() {
    run_path_test_stdout_0("mount-tmp-locally", &[]);
}

#[test]
fn test_fs_mount() {
    let wasm = run_build_script(file!(), "fs-mount").unwrap();
    let test_dir = wasm.parent().unwrap();
    run_wasm_with_runner_config_checked(&wasm, test_dir, |runner| {
        runner.with_mapped_directories([MappedDirectory {
            guest: "/mount".to_string(),
            host: test_dir.to_path_buf(),
        }]);
    })
    .unwrap();
}

#[test]
fn test_open_under_file() {
    run_path_test_stdout_0("open-under-file", &["parent"]);
}

#[test]
fn test_symlink_open_read_write() {
    let wasm = run_build_script(file!(), "symlink-open-read-write").unwrap();
    let test_dir = wasm.parent().unwrap();
    for path in ["hello", "nested", "target.txt"] {
        remove_path_if_exists(&test_dir.join(path));
    }
    fs::write(test_dir.join("target.txt"), "host-prefix:").unwrap();

    let result = run_wasm_with_result(&wasm, test_dir).unwrap();

    assert_eq!(String::from_utf8_lossy(&result.stdout).trim(), "0");
    assert_eq!(result.exit_code, Some(0));
    assert_eq!(
        fs::read_to_string(test_dir.join("target.txt")).unwrap(),
        "host-prefix: bla"
    );

    for path in ["hello", "nested", "target.txt"] {
        remove_path_if_exists(&test_dir.join(path));
    }
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
