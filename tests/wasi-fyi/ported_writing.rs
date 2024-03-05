// WASI:
// mapdir: act1:test_fs/hamlet/act1
// mapdir: act2:test_fs/hamlet/act2
// mapdir: act1-again:test_fs/hamlet/act1

use std::fs;
use std::io::Write;

pub const BYTE_STR: &'static [u8] = b"abcdefghijklmnopqrstuvwxyz";

fn main() {
    do_logic_on_path("/hamlet/act1/abc", "/hamlet/act1/abc");
}

fn do_logic_on_path(path: &'static str, alt_path: &'static str) {
    {
        let mut f = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(path)
            .unwrap();
        f.write_all(BYTE_STR).unwrap();
    }

    println!("{}", fs::read_to_string(alt_path).unwrap());
    fs::remove_file(path).unwrap();

    let file_path = std::path::Path::new(path);
    if file_path.exists() {
        println!("file is here");
    } else {
        println!("file is gone")
    }
}
