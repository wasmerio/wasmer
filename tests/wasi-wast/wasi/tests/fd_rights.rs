use std::io::{Read, Write};

fn main() {
    println!("0");
    let readdir = std::fs::read_dir("/").unwrap();
    let files = readdir.collect::<Vec<_>>();
    assert!(files.is_empty());
    println!("1");

    let mut filehandle = std::fs::OpenOptions::new()
        .read(false) // <- should only be writeable, not readable
        .write(true)
        .create(true)
        .open("foo.txt")
        .unwrap();
    println!("1.5");
    filehandle.write_all(b"test");
    println!("2");

    let mut contents = String::new();
    assert!(filehandle.read_to_string(&mut contents).is_err());
    println!("3");
}
