// WASI:
// mapdir: .:test_fs/hamlet

use std::fs;

fn main() {
    std::env::set_current_dir("/hamlet").unwrap();

    let read_dir = fs::read_dir(".").unwrap();
    let mut out = vec![];
    for entry in read_dir {
        out.push(format!("{:?}", entry.unwrap().path()));
    }
    out.sort();

    for p in out {
        println!("{}", p);
    }
}
