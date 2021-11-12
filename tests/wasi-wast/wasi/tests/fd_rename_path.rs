// WASI:
// dir: test_fs

use std::fs;
use std::path::PathBuf;

fn main() {
    let old_path = PathBuf::from("test_fs/wasitests/dirtorename");
    let new_path = PathBuf::from("test_fs/wasitests/dirrenamed");
    // Clean the test environment
    let _ = fs::remove_dir(&old_path);
    let _ = fs::remove_dir(&new_path);

    fs::create_dir_all(&old_path).expect("cannot create the directory");

    // Doesn't properly work on macOS.
    // fs::rename(old_path, &new_path).expect("cannot rename the directory");
    fs::remove_dir(&new_path).expect("cannot remove the directory");
}
