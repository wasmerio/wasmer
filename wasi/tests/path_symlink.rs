// WASI:
// tempdir: temp
// mapdir: hamlet:test_fs/hamlet

use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

fn main() {
    #[cfg(not(target_os = "wasi"))]
    let mut base = PathBuf::from("test_fs");
    #[cfg(target_os = "wasi")]
    let mut base = PathBuf::from("/");

    let symlink_loc = base.join("temp/act3");
    let symlink_target = "../hamlet/act3";
    let scene1 = symlink_loc.join("scene1.txt");

    std::fs::soft_link(&symlink_target, &symlink_loc);

    let mut file = fs::File::open(&scene1).expect("Could not open file");

    let mut buffer = [0u8; 64];

    assert_eq!(file.read(&mut buffer).unwrap(), 64);
    let str_val = std::str::from_utf8(&buffer[..]).unwrap();
    println!("{}", str_val);

    std::fs::remove_file(symlink_loc).unwrap();
}
