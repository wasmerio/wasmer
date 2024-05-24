use std::fs::OpenOptions;
use std::io::prelude::*;
use std::io::SeekFrom;

fn main() {
    let mut file = OpenOptions::new()
        .append(true)
        .read(true)
        .create(true)
        .open("file")
        .unwrap();

    // file offset must be 1 now
    write!(file, "{}", "a").unwrap();

    // rewind should not work on file in append mode
    // since the offset must always be at the end of the file
    let _ = file.rewind();

    // file offset must be 2 now
    write!(file, "{}", "b").unwrap();

    assert_eq!(file.seek(SeekFrom::Current(0)).unwrap(), 2);
}
