use std::fs;

use super::{
    MappedDirectory, run_build_script, run_wasm_with_result, run_wasm_with_runner_config_checked,
};

fn run_path_test_stdout_0(test_name: &str) {
    let wasm = run_build_script(file!(), test_name).unwrap();
    let result = run_wasm_with_result(&wasm, wasm.parent().unwrap()).unwrap();
    assert_eq!(String::from_utf8_lossy(&result.stdout).trim(), "0");
    assert_eq!(result.exit_code, Some(0));
}

fn run_path_test_stdout_0_in_temp_dir(test_name: &str) {
    let wasm = run_build_script(file!(), test_name).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let result = run_wasm_with_result(&wasm, temp.path()).unwrap();
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
    run_path_test_stdout_0("closing-pre-opened-dirs");
}

#[test]
fn test_create_and_remove_dirs() {
    run_path_test_stdout_0_in_temp_dir("create-and-remove-dirs");
}

#[test]
fn test_create_dir_at_cwd() {
    run_path_test_stdout_0_in_temp_dir("create-dir-at-cwd");
}

#[test]
fn test_create_dir_at_cwd_with_chdir() {
    run_path_test_stdout_0_in_temp_dir("create-dir-at-cwd-with-chdir");
}

#[test]
fn test_cwd_to_home() {
    run_path_test_stdout_0("cwd-to-home");
}

#[test]
fn test_distinct_inodes_same_basename() {
    run_path_test_stdout_0_in_temp_dir("distinct-inodes-same-basename");
}

#[test]
fn test_fstatat_with_chdir() {
    run_path_test_stdout_0_in_temp_dir("fstatat-with-chdir");
}

#[test]
fn test_mount_tmp_locally() {
    run_path_test_stdout_0("mount-tmp-locally");
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
    run_path_test_stdout_0_in_temp_dir("open-under-file");
}

#[test]
fn test_symlink_open_read_write() {
    let wasm = run_build_script(file!(), "symlink-open-read-write").unwrap();
    let temp = tempfile::tempdir().unwrap();
    let test_dir = temp.path();
    fs::write(test_dir.join("target.txt"), "host-prefix:").unwrap();

    let result = run_wasm_with_result(&wasm, test_dir).unwrap();

    assert_eq!(String::from_utf8_lossy(&result.stdout).trim(), "0");
    assert_eq!(result.exit_code, Some(0));
    assert_eq!(
        fs::read_to_string(test_dir.join("target.txt")).unwrap(),
        "host-prefix: bla"
    );
}

#[test]
fn test_create_move_open() {
    let wasm = run_build_script(file!(), "create-move-open").unwrap();
    let temp = tempfile::tempdir().unwrap();
    let result = run_wasm_with_result(&wasm, temp.path()).unwrap();
    assert_eq!(String::from_utf8_lossy(&result.stdout).trim(), "0");
    assert_eq!(result.exit_code, Some(0));
}
