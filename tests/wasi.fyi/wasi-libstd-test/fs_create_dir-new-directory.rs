use std::fs;

fn main() {
  assert!(fs::create_dir("fs_create_dir-new-directory.dir/new_directory").is_ok());
}
