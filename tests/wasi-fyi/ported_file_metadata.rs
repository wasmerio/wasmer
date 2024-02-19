// WASI:
// dir: test_fs

use std::fs;

fn main() {
    let this_file = fs::File::open("/hamlet/act1/scene1.txt").expect("could not find file");
    let md = this_file.metadata().unwrap();
    println!("is dir: {}", md.is_dir());
    let filetype = md.file_type();
    println!(
        "filetype: {} {} {}",
        filetype.is_dir(),
        filetype.is_file(),
        filetype.is_symlink()
    );
    println!("file info: {}", md.len());
}
