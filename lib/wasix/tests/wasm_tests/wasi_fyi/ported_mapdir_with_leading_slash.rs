//#AbstractConfigFile: wasi-fyi.config
//#ExpectedStdoutFile: ported_mapdir_with_leading_slash.stdout
// WASI:
// mapdir: /hamlet:test_fs/hamlet

use std::fs;
use std::io::Read;
use std::path::PathBuf;

fn main() {
    let mut base = PathBuf::from("/hamlet");

    base.push("act3/scene3.txt");

    println!("File exists? {}", base.exists());

    let mut f = fs::File::open(&base).unwrap();
    let mut s = String::new();
    f.read_to_string(&mut s).unwrap();

    println!("{}", s.chars().take(252).collect::<String>());
}
