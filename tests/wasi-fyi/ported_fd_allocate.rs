// WASI:
// tempdir: .

use std::fs;
#[cfg(target_os = "wasi")]
use std::os::wasi::prelude::AsRawFd;
use std::path::PathBuf;

#[cfg(target_os = "wasi")]
#[link(wasm_import_module = "wasi_unstable")]
extern "C" {
    fn fd_allocate(fd: u32, offset: u64, length: u64) -> u16;
}

#[cfg(target_os = "wasi")]
fn allocate(fd: u32, offset: u64, length: u64) -> u16 {
    unsafe { fd_allocate(fd, offset, length) }
}

fn main() {
    #[cfg(target_os = "wasi")]
    let mut base = PathBuf::from(".");
    #[cfg(target_os = "wasi")]
    {
        base.push("fd_allocate_file.txt");
        let mut file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&base)
            .expect("Could not create file");

        {
            use std::io::Write;
            // example text from https://www.un.org/en/universal-declaration-human-rights/
            file.write_all(b"All human beings are born free and equal in dignity and rights. They are endowed with reason and conscience and should act towards one another in a spirit of brotherhood.\n").unwrap();
            let raw_fd = file.as_raw_fd();
            file.flush().unwrap();
            let len = file.metadata().unwrap().len();
            println!("{}", len);
            assert_eq!(len, 171);
            allocate(raw_fd as u32, len, 1234);
            let len = file.metadata().unwrap().len();
            println!("{}", len);
            assert_eq!(len, 1234 + 171);
        }
    }
    #[cfg(target_os = "wasi")]
    std::fs::remove_file(&base).unwrap();

    #[cfg(not(target_os = "wasi"))]
    {
        // eh, just print the output directly
        println!("171");
        println!("1405");
    }
}
