// WASI:
// dir: test_fs

use std::fs;
use std::path::PathBuf;

fn main() {
    let mut idx = 0;
    fs::create_dir_all(PathBuf::from("test_fs/wasitests"))
        .expect("cannot create the parent directory");

    let old_path = loop {
        let old_path = PathBuf::from(format!("test_fs/wasitests/dirtorename-{}", idx));
        if fs::create_dir(old_path.clone()).ok().is_some() {
            break old_path;
        }
        idx += 1;
        if idx > 10 {
            panic!("too many try at creating the folder");
        }
    };

    let new_path = PathBuf::from(format!("test_fs/wasitests/dirrenamed-{}", idx));

    fs::rename(old_path, &new_path).expect("cannot rename the directory");

    fs::remove_dir(&new_path).expect("cannot remove the directory");
}
