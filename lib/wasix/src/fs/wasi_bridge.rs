
use std::sync::Arc;

use vfs_core::{VfsDirHandleAsync, VfsHandleAsync};
use wasmer_wasix_types::wasi::{Errno, Fd as WasiFd, Fdflags, Fdflagsext, Rights};

use super::fd_table::Kind;
use super::vfs::WasiFs;

pub fn insert_vfs_file_fd(
    fs: &WasiFs,
    handle: Arc<VfsHandleAsync>,
    rights: Rights,
    rights_inheriting: Rights,
    flags: Fdflags,
    fd_flags: Fdflagsext,
) -> Result<WasiFd, Errno> {
    fs.create_fd(
        rights,
        rights_inheriting,
        flags,
        fd_flags,
        Kind::VfsFile { handle },
    )
}

pub fn insert_vfs_dir_fd(
    fs: &WasiFs,
    handle: VfsDirHandleAsync,
    rights: Rights,
    rights_inheriting: Rights,
    flags: Fdflags,
    fd_flags: Fdflagsext,
) -> Result<WasiFd, Errno> {
    fs.create_fd(
        rights,
        rights_inheriting,
        flags,
        fd_flags,
        Kind::VfsDir { handle },
    )
}
