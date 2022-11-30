// WASI:
// tempdir: .

use std::io::{Read, Write};
use std::path::PathBuf;

fn main() {
    let file = {
        let mut base = PathBuf::from(".");

        base.push("foo.txt");
        base
    };

    let mut filehandle = std::fs::OpenOptions::new()
        .read(false) // <- should only be writeable, not readable
        .write(true)
        .create(true)
        .open(&file)
        .unwrap();
    filehandle.write_all(b"test");

    let mut contents = String::new();
    assert!(filehandle.read_to_string(&mut contents).is_err());

    std::fs::remove_file(&file).unwrap();
}
