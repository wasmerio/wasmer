use std::fs;

fn main() {
    assert!(fs::File::create("/fyi/fs_file_create.dir/new_file").is_ok());
    assert!(fs::metadata("/fyi/fs_file_create.dir/new_file")
        .unwrap()
        .is_file());
}
