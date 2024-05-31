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

const ERRNO_NOENT: i32 = 44;

fn main() {
    unsafe {
        let errno = path_open(5, 0, 0, 0, 0, 0, 0, 0, 1024);
        assert_eq!(
            errno, ERRNO_NOENT,
            "empty path should not resolve successfully"
        );
    }
}
