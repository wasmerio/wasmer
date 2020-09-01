// WASI:
// mapdir: /dev:/dev

use std::fs;
use std::io::Write;

fn main() {
    let mut f = fs::OpenOptions::new()
        .write(true)
        .open("/dev/null")
        .unwrap();
    let result = f.write(b"hello, world!").unwrap();
    println!("{}", result);
}
