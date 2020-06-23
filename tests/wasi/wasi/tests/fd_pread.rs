// WASI:
// dir: test_fs

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
    fn fd_pread(fd: u32, iovs: u32, iovs_len: u32, offset: u64, nread: u32) -> u16;
}

#[cfg(target_os = "wasi")]
fn pread(fd: u32, iovs: &[&mut [u8]], offset: u64) -> u32 {
    let mut nread = 0;
    let mut processed_iovs = vec![];

    for iov in iovs {
        processed_iovs.push(WasiIovec {
            buf: iov.as_ptr() as usize as u32,
            buf_len: iov.len() as u32,
        })
    }

    unsafe {
        fd_pread(
            fd,
            processed_iovs.as_ptr() as usize as u32,
            processed_iovs.len() as u32,
            offset,
            &mut nread as *mut u32 as usize as u32,
        );
    }
    nread
}

fn main() {
    let mut base = PathBuf::from("test_fs/hamlet");

    base.push("act3/scene4.txt");
    let mut file = fs::File::open(&base).expect("Could not open file");
    let mut buffer = [0u8; 64];

    #[cfg(target_os = "wasi")]
    {
        let raw_fd = file.as_raw_fd();
        assert_eq!(pread(raw_fd, &[&mut buffer], 75), 64);
        let str_val = std::str::from_utf8(&buffer[..]).unwrap().to_string();
        println!("{}", &str_val);
        for i in 0..buffer.len() {
            buffer[i] = 0;
        }
        assert_eq!(pread(raw_fd, &[&mut buffer], 75), 64);
        let str_val2 = std::str::from_utf8(&buffer[..]).unwrap().to_string();
        println!("{}", &str_val2);

        println!("Read the same data? {}", str_val == str_val2);
    }

    #[cfg(not(target_os = "wasi"))]
    {
        // eh, just print the output directly
        println!(
            " POLONIUS

    He will come straight. Look you lay home to him:

 POLONIUS

    He will come straight. Look you lay home to him:

Read the same data? true"
        );
    }
}
