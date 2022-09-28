use std::io::{Read, Write};

fn main() {
    let readdir = std::fs::read_dir("/").unwrap();
    let files = readdir.collect::<Vec<_>>();
    assert!(files.is_empty());

    let mut filehandle = std::fs::OpenOptions::new()
        .read(false) // <- should only be writeable, not readable
        .write(true)
        .create(true)
        .open("foo.txt")
        .unwrap();
    
    filehandle.write_all(b"test");

    let mut contents = String::new();
    assert!(filehandle.read_to_string(&mut contents).is_err());
}