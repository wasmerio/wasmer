use std::os::fd::AsRawFd;

#[link(wasm_import_module = "wasi_snapshot_preview1")]
extern "C" {
    pub fn fd_advise(arg0: i32, arg1: u64, arg2: u64, arg3: i32) -> i32;
}

const ERRNO_SUCCESS: i32 = 0;
const ERRNO_BADF: i32 = 8;

const ADVISE_WILLNEED: i32 = 3;

fn main() {
    unsafe {
        let errno = fd_advise(9999, 0, 0, ADVISE_WILLNEED);
        assert_eq!(
            errno, ERRNO_BADF,
            "fd_advise for invalid file descriptor should have failed with errno 8 (BADF)"
        );

        let f = std::fs::File::create("test.sh").unwrap();

        let errno = fd_advise(f.as_raw_fd(), u64::MAX, u64::MAX, ADVISE_WILLNEED);
        assert_eq!(
            errno, ERRNO_SUCCESS,
            "fd_advise with valid fd should succeed"
        );
    }
}
