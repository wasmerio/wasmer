// WASI:
// mapdir: .:test_fs/hamlet

use std::fs;
#[cfg(target_os = "wasi")]
use std::os::wasi::prelude::AsRawFd;
use std::path::PathBuf;

#[cfg(target_os = "wasi")]
#[link(wasm_import_module = "wasi_unstable")]
extern "C" {
    fn fd_close(fd: u32) -> u16;
}

fn main() {
    #[cfg(not(target_os = "wasi"))]
    let mut base = PathBuf::from("test_fs/hamlet");
    #[cfg(target_os = "wasi")]
    let mut base = PathBuf::from("/hamlet");

    base.push("act3/scene3.txt");
    let file = fs::File::open(&base).expect("could not open file");

    #[cfg(target_os = "wasi")]
    {
        let file_fd = file.as_raw_fd() as u32;
        let stdout_fd = std::io::stdout().as_raw_fd() as u32;
        let stderr_fd = std::io::stderr().as_raw_fd() as u32;
        let stdin_fd = std::io::stdin().as_raw_fd() as u32;

        let result = unsafe { fd_close(file_fd) };
        if result == 0 {
            println!("Successfully closed file!")
        } else {
            println!("Could not close file");
        }

        let result = unsafe { fd_close(stderr_fd) };
        if result == 0 {
            println!("Successfully closed stderr!")
        } else {
            println!("Could not close stderr");
        }
        let result = unsafe { fd_close(stdin_fd) };
        if result == 0 {
            println!("Successfully closed stdin!")
        } else {
            println!("Could not close stdin");
        }
        let result = unsafe { fd_close(stdout_fd) };
        if result == 0 {
            println!("Successfully closed stdout!")
        } else {
            println!("Could not close stdout");
        }
    }
    #[cfg(not(target_os = "wasi"))]
    {
        println!("Successfully closed file!");
        println!("Successfully closed stderr!");
        println!("Successfully closed stdin!");
    }
}
