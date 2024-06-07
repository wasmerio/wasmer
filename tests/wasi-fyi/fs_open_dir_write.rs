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

const ERRNO_ISDIR: i32 = 31;
const RIGHTS_FD_WRITE: i64 = 64;

fn main() {
    unsafe {
        let base_fd = 5;
        let path = "fyi/fs_open_dir_write.dir";
        let errno = path_open(
            base_fd,
            0,
            path.as_ptr() as i32,
            path.as_bytes().len() as i32,
            0,
            RIGHTS_FD_WRITE,
            0,
            0,
            1024,
        );
        assert_eq!(
            errno, ERRNO_ISDIR,
            "opening a dir with rights::fd_write should fail"
        );
    }
}
