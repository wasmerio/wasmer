// WASI:
// tempdir: .

use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::PathBuf;

static STR1: &str = "Hello, world!\n";
static STR2: &str = "Goodbye, world!\n";

fn main() {
    let file = {
        #[cfg(not(target_os = "wasi"))]
        let mut base = PathBuf::from("test_fs/temp");
        #[cfg(target_os = "wasi")]
        let mut base = PathBuf::from(".");

        base.push("fd_append_test");
        base
    };

    {
        let mut file_handle = OpenOptions::new()
            .create_new(true)
            .append(true)
            .open(&file)
            .expect("Couldn't create file");
        file_handle.write_all(STR1.as_bytes()).unwrap();
        file_handle.flush().unwrap();
        file_handle.sync_all();
    }
    {
        let mut file_handle = OpenOptions::new()
            .append(true)
            .open(&file)
            .expect("Couldn't reopen file to append");
        file_handle.write_all(STR2.as_bytes()).unwrap();
        file_handle.flush().unwrap();
        file_handle.sync_all();
    }

    {
        let mut file_handle = OpenOptions::new()
            .read(true)
            .open(&file)
            .expect("Couldn't reopen file to read");

        let mut test = String::new();
        file_handle.read_to_string(&mut test);

        assert_eq!(&test, &format!("{}{}", STR1, STR2));
        println!("{:?}", &test);
    }
    std::fs::remove_file(&file).unwrap();
}
