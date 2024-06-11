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
const ERRNO_ISDIR: i32 = 31;
const ERRNO_NOTDIR: i32 = 54;
const OFLAGS_CREAT: i32 = 1;
const RIGHTS_FD_READ: i64 = 2;
const RIGHTS_FD_WRITE: i64 = 64;

fn main() {
    unsafe {
        let fd = 5;
        let path_ok = "fyi/fs_open_trailing_slash.dir/file";
        let path_bad = "fyi/fs_open_trailing_slash.dir/file/";
        let path_bad_new_file = "fyi/fs_open_trailing_slash.dir/new-file/";
        let errno = path_open(
            fd,
            0,
            path_ok.as_ptr() as i32,
            path_ok.len() as i32,
            0,
            RIGHTS_FD_READ,
            0,
            0,
            1024,
        );
        assert_eq!(
            errno, ERRNO_SUCCESS,
            "opening a file without a trailing slash works"
        );

        let errno = path_open(
            fd,
            0,
            path_bad.as_ptr() as i32,
            path_bad.len() as i32,
            0,
            RIGHTS_FD_READ,
            0,
            0,
            1024,
        );
        assert_eq!(
            errno, ERRNO_NOTDIR,
            "opening a regular file with a trailing slash should fail"
        );

        let errno = path_open(
            fd,
            0,
            path_bad_new_file.as_ptr() as i32,
            path_bad_new_file.len() as i32,
            OFLAGS_CREAT,
            RIGHTS_FD_READ | RIGHTS_FD_WRITE,
            0,
            0,
            1024,
        );
        assert_eq!(
            errno, ERRNO_ISDIR,
            "creating a regular file with a trailing slash should fail"
        );
    }
}
