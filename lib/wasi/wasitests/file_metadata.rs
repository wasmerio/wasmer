use std::fs;
use std::io::Read;

fn main() {
    let mut this_file =
        fs::File::open("wasitests/file_metadata.rs").expect("could not find src file");
    let md = this_file.metadata().unwrap();
    println!("is dir: {}", md.is_dir());
    println!(
        "file info: {:?} {} {:?} {:?} {:?}",
        md.file_type(),
        md.len(),
        md.modified(),
        md.created(),
        md.accessed()
    );
}
