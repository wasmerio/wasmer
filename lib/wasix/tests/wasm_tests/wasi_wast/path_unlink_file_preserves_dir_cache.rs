//#AbstractConfigFile: wasi-wast.config
//#Toolchains: wasip1
//#MappedDirectory: $temp:/data
//#ExpectedStdoutFile: path_unlink_file_preserves_dir_cache.stdout

use std::fs;
use std::path::Path;

fn main() {
    // wasmer issue #7075: path_unlink_file on a directory must fail with
    // EISDIR without evicting the directory from the parent's cached
    // entries - otherwise a following path_remove_directory reports ENOENT
    // for a directory that still exists.
    let dir = Path::new("/data/victim");
    fs::create_dir_all(dir.join("child")).unwrap();
    fs::write(dir.join("child/file.txt"), b"preserve this file\n").unwrap();

    let err = fs::remove_file(dir).unwrap_err();
    assert_eq!(
        err.raw_os_error(),
        Some(31),
        "unlink of a directory must fail with EISDIR, got {err:?}"
    );

    let err = fs::remove_dir(dir).unwrap_err();
    assert_eq!(
        err.raw_os_error(),
        Some(55),
        "rmdir of a non-empty directory must fail with ENOTEMPTY, got {err:?}"
    );

    assert!(
        dir.join("child/file.txt").exists(),
        "directory contents must be preserved"
    );
    println!("Success");
}
