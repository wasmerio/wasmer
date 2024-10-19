use std::os::fd::AsRawFd;

#[link(wasm_import_module = "wasi_snapshot_preview1")]
extern "C" {
    pub fn fd_seek(fd: i32, offset: i64, whence: i32, filesize: i32) -> i32;
}

const ERRNO_SUCCESS: i32 = 0;
const WHENCE_SET: i32 = 0;

fn main() {
    unsafe {
        let offset = 100u64;
        let mut new_offset = 0u64;
        let f = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .append(true)
            .open("/fyi/fs_seek_append.dir/file")
            .unwrap();
        let errno = fd_seek(
            f.as_raw_fd(),
            offset as i64,
            WHENCE_SET,
            &mut new_offset as *mut u64 as usize as i32,
        );

        assert_eq!(errno, ERRNO_SUCCESS);
        assert_eq!(offset, new_offset);
    }
}
