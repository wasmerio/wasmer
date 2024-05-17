use std::fs;

fn main() {
    assert!(fs::create_dir_all("/fyi/fs_create_dir_all-new-directories.dir/new_directory").is_ok());
    assert!(fs::remove_dir_all("/fyi/fs_create_dir_all-new-directories.dir/new_directory").is_ok());
}
