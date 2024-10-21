use std::io::{Seek as _, SeekFrom};

fn main() {
    let offset = 100u64;
    let mut f = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .append(true)
        .open("/fyi/fs_seek_append.dir/file")
        .unwrap();
    let new_offset = f.seek(SeekFrom::Start(offset)).unwrap();

    assert_eq!(offset, new_offset);
}
