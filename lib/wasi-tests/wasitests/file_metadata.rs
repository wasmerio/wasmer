// Args:
// dir: .

use std::fs;
use std::io::Read;

fn main() {
    let mut this_file =
        fs::File::open("wasitests/file_metadata.rs").expect("could not find src file");
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
