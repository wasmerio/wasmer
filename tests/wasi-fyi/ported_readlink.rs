// WASI:
// dir: test_fs

use std::io::Read;

fn main() {
    let sym_link_path = "/hamlet/bookmarks/2019-07-16";

    let link_path = std::fs::read_link(sym_link_path).expect("Could not read link");
    println!("{}", link_path.to_string_lossy());

    let mut some_contents =
        std::fs::File::open(sym_link_path).expect("Could not open file via symlink");

    let mut buffer = [0; 128];

    assert_eq!(
        some_contents
            .read(&mut buffer)
            .expect("Could not read 128 bytes from file"),
        128
    );
    let str_val = std::str::from_utf8(&buffer[..]).expect("Could not parse buffer bytes as UTF-8");
    println!("{}", str_val);
}
