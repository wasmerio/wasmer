use std::fs;

fn main() {
    let old_path = "/fyi/fs_rename-directory.dir/old_directory";
    let new_path = "/fyi/fs_rename-directory.dir/new_directory";

    assert!(fs::rename(old_path, new_path).is_ok());
}
