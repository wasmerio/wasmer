use std::fs;

fn main() {
  let metadata = fs::metadata("fs_metadata-directory.dir/directory").unwrap();
  assert!(metadata.is_dir());
}