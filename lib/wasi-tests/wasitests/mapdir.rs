// Args:
// mapdir: .:wasitests/test_fs/hamlet

use std::fs;

fn main() {
    #[cfg(not(target_os = "wasi"))]
    let cur_dir = std::env::current_dir().unwrap();
    #[cfg(not(target_os = "wasi"))]
    std::env::set_current_dir("wasitests/test_fs/hamlet").unwrap();

    let read_dir = fs::read_dir(".").unwrap();
    let mut out = vec![];
    for entry in read_dir {
        out.push(format!("{:?}", entry.unwrap().path()));
    }
    out.sort();

    for p in out {
        println!("{}", p);
    }
    // return to the current directory
    #[cfg(not(target_os = "wasi"))]
    std::env::set_current_dir(cur_dir).unwrap();
}
