use std::fs;

fn main() {
    assert!(fs::create_dir_all("/fyi/foo/bar").is_ok());
    assert!(fs::remove_dir_all("/fyi/foo").is_ok());
}