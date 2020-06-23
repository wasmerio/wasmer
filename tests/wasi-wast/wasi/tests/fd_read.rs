// WASI:
// mapdir: .:test_fs/hamlet

// this program is used in the pause/resume test

use std::fs;
#[cfg(target_os = "wasi")]
use std::os::wasi::prelude::AsRawFd;
use std::path::PathBuf;

#[cfg(target_os = "wasi")]
#[repr(C)]
struct WasiIovec {
    pub buf: u32,
    pub buf_len: u32,
}

#[cfg(target_os = "wasi")]
#[link(wasm_import_module = "wasi_unstable")]
extern "C" {
    fn fd_read(fd: u32, iovs: u32, iovs_len: u32, nread: u32) -> u16;
}

#[cfg(target_os = "wasi")]
fn read(fd: u32, iovs: &[&mut [u8]]) -> u32 {
    let mut nread = 0;
    let mut processed_iovs = vec![];

    for iov in iovs {
        processed_iovs.push(WasiIovec {
            buf: iov.as_ptr() as usize as u32,
            buf_len: iov.len() as u32,
        })
    }

    unsafe {
        fd_read(
            fd,
            processed_iovs.as_ptr() as usize as u32,
            processed_iovs.len() as u32,
            &mut nread as *mut u32 as usize as u32,
        );
    }
    nread
}

fn main() {
    #[cfg(not(target_os = "wasi"))]
    let mut base = PathBuf::from("test_fs/hamlet");
    #[cfg(target_os = "wasi")]
    let mut base = PathBuf::from(".");

    base.push("act3/scene4.txt");
    let mut file = fs::File::open(&base).expect("Could not open file");
    let mut buffer = [0u8; 64];

    #[cfg(target_os = "wasi")]
    {
        let raw_fd = file.as_raw_fd();
        assert_eq!(read(raw_fd, &[&mut buffer]), 64);
        let str_val = std::str::from_utf8(&buffer[..]).unwrap().to_string();
        println!("{}", &str_val);
    }
    // leak the file handle so that we can use it later
    std::mem::forget(file);

    #[cfg(not(target_os = "wasi"))]
    {
        // eh, just print the output directly
        println!(
            "SCENE IV. The Queen's closet.

    Enter QUEEN GERTRUDE and POLO"
        );
    }
}

#[cfg(target_os = "wasi")]
#[no_mangle]
fn second_entry() -> bool {
    let raw_fd = 5;
    let mut buffer = [0u8; 8];
    let result = read(raw_fd, &[&mut buffer]);

    &buffer == b"NIUS \n\nL"
}
