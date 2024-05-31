use std::ffi::CString;
use std::fs::File;

#[link(wasm_import_module = "wasi_snapshot_preview1")]
extern "C" {
    pub fn path_open(
        fd: i32,
        dirflags: i32,
        path: i32,
        path_len: i32,
        oflags: i32,
        fs_rights_base: i64,
        fs_rights_inheriting: i64,
        fdflags: i32,
        result_fd: i32,
    ) -> i32;
}

const ERRNO_INVAL: i32 = 28;

fn main() {
    unsafe {
        let fd = 5;
        let path = CString::new("/fyi/should-fail").unwrap();

        let errno = path_open(
            fd,
            0,
            path.as_ptr() as i32,
            path.as_bytes().len() as i32,
            0,
            0,
            0,
            0,
            1024,
        );
        assert_eq!(
            errno, ERRNO_INVAL,
            "open absolute path at WASI level shall fail"
        );

        assert!(File::open("/hamlet/README.md").is_ok());
    }
}
