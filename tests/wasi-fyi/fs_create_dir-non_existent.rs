use std::fs;

fn main() {
    assert!(fs::create_dir("/fyi/fs_create_dir-non_existent.dir/not-exist/new-directory").is_err());
}
