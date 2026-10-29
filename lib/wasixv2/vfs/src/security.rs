//! The hooks a Linux security module has in path operations (`include/linux/security.h`):
//! the credentials an operation runs with carry the module's view of the caller
//! (`cred->security`), and the operations ask it, after their own checks of the mode bits
//! where Linux does.

use std::sync::Arc;

use crate::{dentry::Dentry, mount::Path, types::FileType, types::FsResult};

/// A security module's checks (`security_file_open`, `security_path_*`). Each says why not
/// with an errno, or lets the operation go on.
pub trait Security: Send + Sync {
    /// `security_file_open`: `path` is opened for `access` (`MAY_READ`, `MAY_WRITE`,
    /// `MAY_EXEC` for `execve`'s open), and truncated with `truncate` (`O_TRUNC`,
    /// `security_file_truncate`).
    fn file_open(&self, path: &Path, access: u32, truncate: bool) -> FsResult<()>;

    /// `security_path_mknod`, `security_path_mkdir`, `security_path_symlink`, and `open` with
    /// `O_CREAT` making a file: a new `kind` in `dir`.
    fn path_create(&self, dir: &Path, kind: FileType) -> FsResult<()>;

    /// `security_path_unlink`, `security_path_rmdir`: `victim` goes from `dir`.
    fn path_remove(&self, dir: &Path, victim: &Arc<Dentry>) -> FsResult<()>;

    /// `security_path_link`: `old` gets a new name in `new_dir`.
    fn path_link(&self, old: &Path, new_dir: &Path) -> FsResult<()>;

    /// `security_path_rename`: `old` moves from `old_dir` to `new_dir`, over `new` if there is
    /// one, or swaps places with it with `exchange`.
    fn path_rename(
        &self,
        old_dir: &Path,
        old: &Arc<Dentry>,
        new_dir: &Path,
        new: Option<&Arc<Dentry>>,
        exchange: bool,
    ) -> FsResult<()>;

    /// `security_path_truncate`: `truncate(2)` of `path`.
    fn path_truncate(&self, path: &Path) -> FsResult<()>;
}
