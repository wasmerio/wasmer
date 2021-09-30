// WASI:
// dir: test_fs

use std::fs;
use std::path::PathBuf;
use std::time::Instant;

fn main() {
    let start = Instant::now();
    // Use some time to get a pseudo random number to make name unique and avoid race condition during test
    let old_path = PathBuf::from(format!("test_fs/wasitests/dirtorename-{}", start.elapsed().as_nanos()));
    let new_path = PathBuf::from(format!("test_fs/wasitests/dirrenamed-{}", start.elapsed().as_nanos()));
    // Clean the test environment
    let _ = fs::remove_dir(&old_path);
    let _ = fs::remove_dir(&new_path);

    fs::create_dir_all(&old_path).expect("cannot create the directory");

    fs::rename(old_path, &new_path).expect("cannot rename the directory");
    fs::remove_dir(&new_path).expect("cannot remove the directory");
}
