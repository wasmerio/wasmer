// WASI:
// dir: test_fs

use std::fs;
use std::io::Read;

// NOTE: This program is no longer a quine because we want to sandbox these tests to `test_fs`, in the future
// `test_fs` will be implicit.
fn main() {
    let mut this_file =
        fs::File::open("test_fs/hamlet/act1/scene2.txt").expect("could not find src file");
    let md = this_file.metadata().unwrap();
    let mut in_str = String::new();
    this_file.read_to_string(&mut in_str).unwrap();
    println!("{}", in_str);
}
