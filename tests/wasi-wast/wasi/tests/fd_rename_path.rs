// WASI:
// dir: test_fs

use std::fs;
use std::path::PathBuf;

fn main() {
    let mut idx = 0;
    let old_path = loop {
        let old_path = PathBuf::from(format!("test_fs/wasitests/dirtorename-{}", idx));
        if ! fs::read_dir(old_path.clone()).ok().is_some() {
            break old_path;
        }
        idx+=1;
    };

    let new_path = PathBuf::from(format!("test_fs/wasitests/dirrenamed-{}", idx));

    // Clean the test environment
    let _ = fs::remove_dir(&old_path);
    let _ = fs::remove_dir(&new_path);

    fs::create_dir_all(&old_path).expect("cannot create the directory");

    fs::rename(old_path, &new_path).expect("cannot rename the directory");
    fs::remove_dir(&new_path).expect("cannot remove the directory");
}
