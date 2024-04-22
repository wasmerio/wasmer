use std::os::fd::AsRawFd;

#[link(wasm_import_module = "wasi_snapshot_preview1")]
extern "C" {
    pub fn fd_seek(fd: i32, offset: i64, whence: i32, filesize: i32) -> i32;
}

const ERRNO_INVAL: i32 = 28;

const WHENCE_CUR: i32 = 1;

fn main() {
    unsafe {
        let large_negative_offset = -6551085931117533355;
        let mut filesize = 0u64;

        let f = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open("test.sh")
            .unwrap();
        let errno = fd_seek(
            f.as_raw_fd(),
            large_negative_offset,
            WHENCE_CUR,
            &mut filesize as *mut u64 as usize as i32,
        );

        assert_eq!(errno, ERRNO_INVAL);
    }
}
