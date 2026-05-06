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

const ERRNO_PERM: i32 = 63;
const LOOKUPFLAGS_SYMLINK_FOLLOW: i32 = 1;
const OFLAGS_CREAT: i32 = 1;
const RIGHTS_FD_WRITE: i64 = 64;

fn main() {
    let link_path = "fyi/fs_sandbox_symlink.dir/link";
    let link_path_non_existant = "fyi/fs_sandbox_symlink.dir/link-non-existant";
    let mut fd: i32 = 0;

    unsafe {
        let errno = path_open(
            5,
            LOOKUPFLAGS_SYMLINK_FOLLOW,
            link_path.as_ptr() as i32,
            link_path.len() as i32,
            OFLAGS_CREAT,
            RIGHTS_FD_WRITE,
            0,
            0,
            &mut fd as *mut i32 as i32,
        );
        assert_eq!(errno, ERRNO_PERM, "symlink cannot escape fs sandbox");

        let errno = path_open(
            5,
            LOOKUPFLAGS_SYMLINK_FOLLOW,
            link_path_non_existant.as_ptr() as i32,
            link_path_non_existant.len() as i32,
            OFLAGS_CREAT,
            RIGHTS_FD_WRITE,
            0,
            0,
            &mut fd as *mut i32 as i32,
        );
        assert_eq!(errno, ERRNO_PERM, "symlink cannot escape fs sandbox");
    }
}
