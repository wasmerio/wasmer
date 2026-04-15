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

const ERRNO_NOTDIR: i32 = 54;
const OFLAGS_CREAT: i32 = 1;
const OFLAGS_EXCL: i32 = 4;
const RIGHTS_FD_READ: i64 = 2;
const RIGHTS_FD_WRITE: i64 = 64;

fn main() {
    unsafe {
        let path = "fyi/fs_open_file_as_dir.dir/parent_file/child";
        let errno = path_open(
            5,
            0,
            path.as_ptr() as i32,
            path.len() as i32,
            OFLAGS_CREAT | OFLAGS_EXCL,
            RIGHTS_FD_READ | RIGHTS_FD_WRITE,
            0,
            0,
            1024,
        );
        assert_eq!(
            errno, ERRNO_NOTDIR,
            "opening a path whose parent component is a regular file should return ENOTDIR (54), got {}",
            errno
        );
    }
}
