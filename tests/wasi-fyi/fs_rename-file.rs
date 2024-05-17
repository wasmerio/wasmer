use std::fs;

fn main() {
    let old_path = "/fyi/fs_rename-file.dir/old_file";
    let new_path = "/fyi/fs_rename-file.dir/new_file";

    assert!(fs::rename(old_path, new_path).is_ok());

    let error = fs::metadata(old_path).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);

    let metadata = fs::metadata(new_path).unwrap();
    assert!(metadata.is_file());

    assert!(fs::rename(new_path, old_path).is_ok());
}
