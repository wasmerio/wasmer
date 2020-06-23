// WASI:
// dir: test_fs

use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::*;

fn main() {
    let mut path = PathBuf::from("test_fs/wasitests/testing/nested/directories");
    let test_file = path.join("test.file");
    fs::create_dir_all(&path).unwrap();
    {
        let mut file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .truncate(true)
            .create(true)
            .open(&test_file)
            .unwrap();

        assert_eq!(file.write(b"hello").unwrap(), 5);

        file.flush().unwrap();
        file.seek(SeekFrom::Start(0)).unwrap();
        let mut in_str = String::new();
        file.read_to_string(&mut in_str).unwrap();
        assert_eq!(&in_str, "hello");
    }
    fs::remove_file(&test_file).unwrap();
    println!("Test file exists: {}", test_file.exists());
    assert!(!test_file.exists());
    for _ in 0..3 {
        fs::remove_dir_all(&path).unwrap();
        println!("Dir exists: {}", path.exists());
        assert!(!path.exists());
        path.pop();
    }

    println!("Success");
}
