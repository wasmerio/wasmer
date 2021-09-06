// WASI:
// dir: test_fs

use std::fs;

fn main() {
  let old_path = "test_fs/oldname";
  let new_path = "test_fs/newname";

  assert!(fs::rename(old_path, new_path).is_ok());
}
