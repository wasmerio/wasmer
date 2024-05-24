use std::fs::{metadata, OpenOptions};
use std::io::{Seek, SeekFrom, Write};

fn main() {
    let mut file = OpenOptions::new()
        .write(true)
        .read(true)
        .create(true)
        .open("file")
        .unwrap();

    write!(file, "hell").unwrap();

    // We wrote 4 bytes
    let md = metadata("file").unwrap();
    assert_eq!(md.len(), 4);

    assert_eq!(file.seek(SeekFrom::Start(0)).unwrap(), 0);

    write!(file, "eh").unwrap();

    // We overwrote the first 2 bytes, should still have 4 bytes
    let md = metadata("file").unwrap();
    assert_eq!(md.len(), 4);

    assert_eq!(file.seek(SeekFrom::Start(0)).unwrap(), 0);

    write!(file, "hello").unwrap();

    // Now we wrote past the end, should have 5 bytes
    let md = metadata("file").unwrap();
    assert_eq!(md.len(), 5);

    write!(file, " world!").unwrap();

    // We wrote past the end entirely, should have 5 + 7 = 12 bytes
    let md = metadata("file").unwrap();
    assert_eq!(md.len(), 12);
}
