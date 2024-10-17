use std::fs;

fn main() {
    assert!(
        fs::create_dir_all("/fyi/fs_create_dir-existing-directory.dir/existing_directory").is_ok()
    );
}
