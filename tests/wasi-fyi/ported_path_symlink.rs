// WASI:
// tempdir: temp
// mapdir: hamlet:test_fs/hamlet

use std::fs;
use std::io::Read;
use std::path::PathBuf;

fn main() {
    let base = PathBuf::from("/hamlet");

    let symlink_loc = base.join("/tmp/act3");
    let symlink_target = "/hamlet/act3";
    let scene1 = symlink_loc.join("scene1.txt");

    #[allow(deprecated)]
    std::fs::soft_link(&symlink_target, &symlink_loc).unwrap();

    let mut file = fs::File::open(&scene1).expect("Could not open file");

    let mut buffer = [0u8; 64];

    assert_eq!(file.read(&mut buffer).unwrap(), 64);
    let str_val = std::str::from_utf8(&buffer[..]).unwrap();
    println!("{}", str_val);

    std::fs::remove_file(symlink_loc).unwrap();
}
