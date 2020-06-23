// WASI:
// mapdir: .:test_fs/hamlet

use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

fn main() {
    #[cfg(not(target_os = "wasi"))]
    let mut base = PathBuf::from("test_fs/hamlet");
    #[cfg(target_os = "wasi")]
    let mut base = PathBuf::from(".");

    base.push("act1/scene3.txt");

    let mut file = fs::File::open(&base).expect("Could not open file");

    let mut buffer = [0u8; 32];

    assert_eq!(file.read(&mut buffer).unwrap(), 32);
    let str_val = std::str::from_utf8(&buffer[..]).unwrap();
    println!("{}", str_val);

    assert_eq!(file.read(&mut buffer).unwrap(), 32);
    let str_val = std::str::from_utf8(&buffer[..]).unwrap();
    println!("{}", str_val);

    assert_eq!(file.seek(SeekFrom::Start(123)).unwrap(), 123);
    assert_eq!(file.read(&mut buffer).unwrap(), 32);
    let str_val = std::str::from_utf8(&buffer[..]).unwrap();
    println!("{}", str_val);

    assert_eq!(file.seek(SeekFrom::End(-123)).unwrap(), 6617);
    assert_eq!(file.read(&mut buffer).unwrap(), 32);
    let str_val = std::str::from_utf8(&buffer[..]).unwrap();
    println!("{}", str_val);

    assert_eq!(file.seek(SeekFrom::Current(-250)).unwrap(), 6399);
    assert_eq!(file.read(&mut buffer).unwrap(), 32);
    let str_val = std::str::from_utf8(&buffer[..]).unwrap();
    println!("{}", str_val);

    assert_eq!(file.seek(SeekFrom::Current(50)).unwrap(), 6481);
    assert_eq!(file.read(&mut buffer).unwrap(), 32);
    let str_val = std::str::from_utf8(&buffer[..]).unwrap();
    println!("{}", str_val);
}
