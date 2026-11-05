//! Miscellaneous inode attributes (`FS_IOC_GETFLAGS`, `FS_IOC_FSGETXATTR`, `file_getattr`):
//! the VFS half of Linux's `fs/file_attr.c`.

use wasixv2_abi::{Errno, FsInodeFlags as Fl, FsXflags as X};

use crate::{
    Vfs,
    cred::Cap,
    mount::Path,
    notify::FsEvent,
    ops::Ctx,
    types::{FileType, FsResult},
};

/// `FS_IMMUTABLE_FL` and `FS_APPEND_FL`: no change at all, or only appending.
pub const FS_IMMUTABLE_FL: u32 = Fl::FS_IMMUTABLE_FL.0;
pub const FS_APPEND_FL: u32 = Fl::FS_APPEND_FL.0;
/// `FS_NOATIME_FL` and `FS_NODUMP_FL`.
pub const FS_NOATIME_FL: u32 = Fl::FS_NOATIME_FL.0;
pub const FS_NODUMP_FL: u32 = Fl::FS_NODUMP_FL.0;

/// `FS_COMMON_FL`: the flags both interfaces have.
const FS_COMMON_FL: u32 = Fl::FS_SYNC_FL.0
    | Fl::FS_IMMUTABLE_FL.0
    | Fl::FS_APPEND_FL.0
    | Fl::FS_NODUMP_FL.0
    | Fl::FS_NOATIME_FL.0
    | Fl::FS_DAX_FL.0
    | Fl::FS_PROJINHERIT_FL.0
    | Fl::FS_VERITY_FL.0;
/// `FS_XFLAG_COMMON`.
const FS_XFLAG_COMMON: u32 = X::FS_XFLAG_SYNC.0
    | X::FS_XFLAG_IMMUTABLE.0
    | X::FS_XFLAG_APPEND.0
    | X::FS_XFLAG_NODUMP.0
    | X::FS_XFLAG_NOATIME.0
    | X::FS_XFLAG_DAX.0
    | X::FS_XFLAG_PROJINHERIT.0
    | X::FS_XFLAG_VERITY.0;
/// `FS_XFLAG_RDONLY_MASK`: reported, never set.
pub const FS_XFLAG_RDONLY_MASK: u32 = X::FS_XFLAG_PREALLOC.0
    | X::FS_XFLAG_HASATTR.0
    | X::FS_XFLAG_VERITY.0
    | X::FS_XFLAG_CASEFOLD.0
    | X::FS_XFLAG_CASENONPRESERVING.0;
/// `FS_XFLAGS_MASK`: every extended flag the interfaces know.
pub const FS_XFLAGS_MASK: u32 = FS_XFLAG_COMMON
    | FS_XFLAG_RDONLY_MASK
    | X::FS_XFLAG_EXTSIZE.0
    | X::FS_XFLAG_COWEXTSIZE.0
    | X::FS_XFLAG_RTINHERIT.0
    | X::FS_XFLAG_NOSYMLINKS.0
    | X::FS_XFLAG_EXTSZINHERIT.0
    | X::FS_XFLAG_REALTIME.0
    | X::FS_XFLAG_NODEFRAG.0
    | X::FS_XFLAG_FILESTREAM.0;

/// The pairs of flags that mean the same in both interfaces (`fileattr_fill_*`).
const SAME: [(u32, u32); 9] = [
    (Fl::FS_IMMUTABLE_FL.0, X::FS_XFLAG_IMMUTABLE.0),
    (Fl::FS_APPEND_FL.0, X::FS_XFLAG_APPEND.0),
    (Fl::FS_SYNC_FL.0, X::FS_XFLAG_SYNC.0),
    (Fl::FS_NOATIME_FL.0, X::FS_XFLAG_NOATIME.0),
    (Fl::FS_NODUMP_FL.0, X::FS_XFLAG_NODUMP.0),
    (Fl::FS_DAX_FL.0, X::FS_XFLAG_DAX.0),
    (Fl::FS_PROJINHERIT_FL.0, X::FS_XFLAG_PROJINHERIT.0),
    (Fl::FS_VERITY_FL.0, X::FS_XFLAG_VERITY.0),
    (Fl::FS_CASEFOLD_FL.0, X::FS_XFLAG_CASEFOLD.0),
];

/// The merged attributes (`struct file_kattr`): `flags` of `FS_IOC_GETFLAGS` and the
/// `fsx_*` fields of `struct fsxattr`, with which of the two the caller set.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FileKattr {
    pub flags: u32,
    pub xflags: u32,
    pub extsize: u32,
    pub nextents: u32,
    pub projid: u32,
    pub cowextsize: u32,
    /// Set through `flags` (`FS_IOC_SETFLAGS`).
    pub flags_valid: bool,
    /// Set through the extended fields (`FS_IOC_FSSETXATTR`, `file_setattr`).
    pub fsx_valid: bool,
}

impl FileKattr {
    /// `fileattr_fill_flags`.
    pub fn from_flags(flags: u32) -> Self {
        let xflags = SAME
            .iter()
            .filter(|(fl, _)| flags & fl != 0)
            .fold(0, |x, (_, xfl)| x | xfl);
        Self {
            flags,
            xflags,
            flags_valid: true,
            ..Self::default()
        }
    }

    /// `fileattr_fill_xflags`.
    pub fn from_xflags(xflags: u32) -> Self {
        let flags = SAME
            .iter()
            .filter(|(_, xfl)| xflags & xfl != 0)
            .fold(0, |f, (fl, _)| f | fl);
        Self {
            flags,
            xflags,
            fsx_valid: true,
            ..Self::default()
        }
    }

    /// `fileattr_has_fsx`: anything set that `flags` cannot say.
    pub fn has_fsx(&self) -> bool {
        self.fsx_valid
            && (self.xflags & !FS_XFLAG_COMMON != 0
                || self.extsize != 0
                || self.projid != 0
                || self.cowextsize != 0)
    }
}

impl Vfs {
    /// `vfs_fileattr_get`: `ENOTTY` for a filesystem without the attributes.
    pub async fn fileattr_get(&self, path: &Path) -> FsResult<FileKattr> {
        path.dentry.node().fileattr_get().await
    }

    /// `vfs_fileattr_set` after `mnt_want_write`: the owner (or `CAP_FOWNER`) changes them,
    /// the immutable and append-only flags only with `CAP_LINUX_IMMUTABLE`; what the caller
    /// did not set keeps its value.
    pub async fn fileattr_set(&self, cx: &Ctx<'_>, path: &Path, mut fa: FileKattr) -> FsResult<()> {
        if path.mnt.is_read_only() {
            return Err(Errno::EROFS);
        }
        let node = path.dentry.node();
        let attr = node.getattr().await?;
        if !cx.cred.owns(&attr) {
            return Err(Errno::EPERM);
        }
        let _lock = path.dentry.inode().lock().await;
        let old = node.fileattr_get().await?;
        if fa.flags_valid {
            fa.xflags |= old.xflags & !FS_XFLAG_COMMON;
            fa.extsize = old.extsize;
            fa.nextents = old.nextents;
            fa.projid = old.projid;
            fa.cowextsize = old.cowextsize;
        } else {
            fa.flags |= old.flags & !FS_COMMON_FL;
        }
        fileattr_set_prepare(cx, attr.file_type, &old, &mut fa)?;
        node.fileattr_set(&fa).await?;
        let object = Self::object(&path.dentry);
        self.notify_object(&path.dentry, FsEvent::Attrib { object });
        Ok(())
    }
}

/// `fileattr_set_prepare`.
fn fileattr_set_prepare(
    cx: &Ctx<'_>,
    file_type: FileType,
    old: &FileKattr,
    fa: &mut FileKattr,
) -> FsResult<()> {
    if (fa.flags ^ old.flags) & (FS_APPEND_FL | FS_IMMUTABLE_FL) != 0
        && !cx.cred.capable_init(Cap::LinuxImmutable)
    {
        return Err(Errno::EPERM);
    }
    // Project ids belong to the initial user namespace.
    if !cx.cred.user_ns.is_initial() {
        if old.projid != fa.projid || (old.xflags ^ fa.xflags) & X::FS_XFLAG_PROJINHERIT.0 != 0 {
            return Err(Errno::EINVAL);
        }
    } else if old.projid != fa.projid && fa.projid == u32::MAX {
        return Err(Errno::EINVAL);
    }
    let regular = file_type == FileType::Regular;
    let dir = file_type == FileType::Directory;
    if fa.xflags & X::FS_XFLAG_EXTSIZE.0 != 0 && !regular {
        return Err(Errno::EINVAL);
    }
    if fa.xflags & X::FS_XFLAG_EXTSZINHERIT.0 != 0 && !dir {
        return Err(Errno::EINVAL);
    }
    if fa.xflags & X::FS_XFLAG_COWEXTSIZE.0 != 0 && !regular && !dir {
        return Err(Errno::EINVAL);
    }
    if fa.xflags & X::FS_XFLAG_DAX.0 != 0
        && (!(regular || dir) || old.xflags & X::FS_XFLAG_VERITY.0 != 0)
    {
        return Err(Errno::EINVAL);
    }
    if fa.extsize == 0 {
        fa.xflags &= !(X::FS_XFLAG_EXTSIZE.0 | X::FS_XFLAG_EXTSZINHERIT.0);
    }
    if fa.cowextsize == 0 {
        fa.xflags &= !X::FS_XFLAG_COWEXTSIZE.0;
    }
    Ok(())
}
