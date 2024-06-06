use std::ffi::CString;
use std::fs;

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

const ERRNO_SUCCESS: i32 = 0;
const ERRNO_EXIST: i32 = 20;
const OFLAGS_CREAT: i32 = 1;
const OFLAGS_EXCL: i32 = 4;
const RIGHTS_FD_READ: i64 = 2;
const RIGHTS_FD_WRITE: i64 = 64;

fn main() {
    unsafe {
        let fd = 5;
        let path0 = CString::new("fyi/fs_open_dir_excl.dir").unwrap();
        let path1 = CString::new("fyi/fs_open_dir_excl.dir/file").unwrap();
        let errno = path_open(
            fd,
            0,
            path0.as_ptr() as i32,
            path0.as_bytes().len() as i32,
            OFLAGS_CREAT | OFLAGS_EXCL,
            RIGHTS_FD_READ | RIGHTS_FD_WRITE,
            0,
            0,
            1024,
        );
        assert_eq!(
            errno, ERRNO_EXIST,
            "opening an existing directory with excl should fail"
        );

        let _ = fs::remove_file("/fyi/fs_open_dir_excl.dir/file");
        let errno = path_open(
            fd,
            0,
            path1.as_ptr() as i32,
            path1.as_bytes().len() as i32,
            OFLAGS_CREAT | OFLAGS_EXCL,
            RIGHTS_FD_READ | RIGHTS_FD_WRITE,
            0,
            0,
            1024,
        );
        assert_eq!(
            errno, ERRNO_SUCCESS,
            "opening a non-existing path with excl should succeed"
        );
    }
}
