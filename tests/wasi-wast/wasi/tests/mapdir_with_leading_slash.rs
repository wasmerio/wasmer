// WASI:
// mapdir: /hamlet:test_fs/hamlet

use std::fs;
use std::io::Read;
use std::path::PathBuf;

fn main() {
    #[cfg(not(target_os = "wasi"))]
    let mut base = PathBuf::from("test_fs/hamlet");
    #[cfg(target_os = "wasi")]
    let mut base = PathBuf::from("hamlet");

    base.push("act3/scene3.txt");

    println!("File exists? {}", base.exists());

    let mut f = fs::File::open(&base).unwrap();
    let mut s = String::new();
    f.read_to_string(&mut s).unwrap();

    println!("{}", s.chars().take(256).collect::<String>());
}
