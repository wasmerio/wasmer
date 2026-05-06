use std::fs;

use super::{
    MappedDirectory, run_build_script, run_wasm_with_result, run_wasm_with_runner_config_checked,
};

wasm_test!(test_chdir_getcwd, "chdir-getcwd", stdout = "0");

wasm_test!(test_closing_pre_opened_dirs, "closing-pre-opened-dirs", stdout = "0");

wasm_test!(test_create_and_remove_dirs, "create-and-remove-dirs", temp_dir, stdout = "0");

wasm_test!(test_create_dir_at_cwd, "create-dir-at-cwd", temp_dir, stdout = "0");

wasm_test!(
    test_create_dir_at_cwd_with_chdir,
    "create-dir-at-cwd-with-chdir",
    temp_dir,
    stdout = "0"
);

wasm_test!(test_cwd_to_home, "cwd-to-home", stdout = "0");

wasm_test!(
    test_distinct_inodes_same_basename,
    "distinct-inodes-same-basename",
    temp_dir,
    stdout = "0"
);

wasm_test!(test_fstatat_with_chdir, "fstatat-with-chdir", temp_dir, stdout = "0");

wasm_test!(test_mount_tmp_locally, "mount-tmp-locally", stdout = "0");

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

wasm_test!(test_open_under_file, "open-under-file", temp_dir, stdout = "0");

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

wasm_test!(test_create_move_open, "create-move-open", temp_dir, stdout = "0");

wasm_test!(test_rename_same_path, "rename-same-path", temp_dir);
