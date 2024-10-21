use std::{
    fs,
    io::{Seek, SeekFrom, Write},
};

fn main() {
    let file = "fyi/fs_open_append_offset.dir/file";
    let mut f0 = fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(file)
        .unwrap();

    f0.write_all(b"abc").unwrap();
    f0.seek(SeekFrom::Start(1)).unwrap();

    assert_eq!(fs::read_to_string(file).unwrap(), "abc");

    // This open with append should not affect the offset of f0.
    let _f1 = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(true)
        .open(file)
        .unwrap();

    f0.write_all(b"d").unwrap();

    assert_eq!(fs::read_to_string(file).unwrap(), "adc");
}
