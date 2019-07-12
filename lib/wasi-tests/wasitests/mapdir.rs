// Args:
// mapdir: .:wasitests/test_fs/hamlet

use std::fs;

fn main() {
    #[cfg(not(target_os = "wasi"))]
    let read_dir = fs::read_dir("wasitests/test_fs/hamlet").unwrap();
    #[cfg(target_os = "wasi")]
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
