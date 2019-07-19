// Args:
// mapdir: .:wasitests/test_fs/hamlet
use std::io::Read;

fn main() {
    #[cfg(not(target_os = "wasi"))]
    std::env::set_current_dir("wasitests/test_fs/hamlet").unwrap();

    let sym_link_path = "bookmarks/2019-07-16";

    let link_path = std::fs::read_link(sym_link_path).unwrap();
    println!("{}", link_path.to_string_lossy());

    let mut some_contents = std::fs::File::open(sym_link_path).unwrap();

    let mut buffer = [0; 128];

    assert_eq!(some_contents.read(&mut buffer).unwrap(), 128);
    let str_val = std::str::from_utf8(&buffer[..]).unwrap();
    println!("{}", str_val);
}
