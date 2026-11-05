//! File handles: the VFS half of Linux's `fs/fhandle.c` and `fs/exportfs/expfs.c`.
//!
//! A filesystem with export operations encodes and decodes its own handles
//! ([`Filesystem::encode_fh`], [`Filesystem::fh_to_node`]); every other one only gives
//! identifiers that are never decoded, made from the inode number and generation, which is
//! what `AT_HANDLE_FID` and fanotify's file identifiers need.
//!
//! [`Filesystem::encode_fh`]: crate::Filesystem::encode_fh
//! [`Filesystem::fh_to_node`]: crate::Filesystem::fh_to_node

use std::sync::Arc;

use wasixv2_abi::{Errno, FileHandleMisc};

use crate::{
    Vfs,
    cred::Cred,
    dentry::Dentry,
    driver::Node,
    mount::Path,
    notify::Object,
    types::{Device, FileId, FileType, FsResult},
};

/// `MAX_HANDLE_SZ`: the longest handle body.
pub const MAX_HANDLE_SZ: usize = FileHandleMisc::MAX_HANDLE_SZ.0 as usize;

/// What a handle is for (`EXPORT_FH_*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandleKind {
    /// A handle `open_by_handle_at` can decode.
    Decodable,
    /// An identifier only (`EXPORT_FH_FID`, `AT_HANDLE_FID`).
    Fid,
    /// A decodable handle that also names the parent (`EXPORT_FH_CONNECTABLE`).
    Connectable,
}

/// `exportfs_encode_ino64_fid`: `struct fid`'s `i64` member, the inode number and generation.
fn ino64_fid(node: &dyn Node) -> FileId {
    let mut data = node.ino().to_le_bytes().to_vec();
    data.extend_from_slice(&node.generation().to_le_bytes());
    FileId {
        kind: FileHandleMisc::FILEID_INO64_GEN.0,
        data,
    }
}

impl Vfs {
    /// The identifier (as `AT_HANDLE_FID` gives it) of the object `object` names, while
    /// anything holds it, and the device of its filesystem, which fanotify reports objects by
    /// (`fanotify_encode_fh`, `vfs_get_fsid`).
    pub fn object_fid(&self, object: Object) -> Option<(FileId, Device)> {
        let sb = self.superblock(object.sb)?;
        let inode = sb.inodes.lock().unwrap().get(&object.ino)?.upgrade()?;
        let node = inode.node().as_ref();
        let fid = sb.fs.encode_fh(node).unwrap_or_else(|| ino64_fid(node));
        Some((fid, sb.dev))
    }

    /// `exportfs_encode_fh`: the handle of the object `path` names. `EOPNOTSUPP` for a
    /// decodable handle of a filesystem without export operations, and for a connectable one,
    /// which needs `fh_to_parent`, which no filesystem here has (as tmpfs).
    pub fn encode_fh(&self, path: &Path, kind: HandleKind) -> FsResult<FileId> {
        let node = path.dentry.node();
        match (kind, path.dentry.sb().fs.encode_fh(node.as_ref())) {
            (HandleKind::Connectable, _) => Err(Errno::EOPNOTSUPP),
            (_, Some(fid)) => Ok(fid),
            (HandleKind::Fid, None) => Ok(ino64_fid(node.as_ref())),
            (HandleKind::Decodable, None) => Err(Errno::EOPNOTSUPP),
        }
    }

    /// `exportfs_decode_fh_raw` on the filesystem `root` is on: the object `fid` names, as a
    /// path on `root`'s mount, through a name it has if the VFS knows one. Everything that
    /// does not decode is `ESTALE` (`do_handle_to_path`). `dir_only` takes only directories
    /// (`EXPORT_FH_DIR_ONLY`); `check` is the caller of the relaxed permission model
    /// (`HANDLE_CHECK_PERMS`), whose user namespace must map the owner of the object and of
    /// every directory above it (`vfs_dentry_acceptable`). `check_subtree` additionally
    /// requires that the ancestry reaches the supplied root (`HANDLE_CHECK_SUBTREE`).
    pub async fn decode_fh(
        &self,
        root: &Path,
        fid: &FileId,
        dir_only: bool,
        check: Option<&Cred>,
        check_subtree: bool,
    ) -> FsResult<Path> {
        let sb = root.dentry.sb();
        let node = sb.fs.fh_to_node(fid).await.map_err(|errno| match errno {
            Errno::ENOMEM => Errno::ENOMEM,
            _ => Errno::ESTALE,
        })?;
        if dir_only && node.file_type() != FileType::Directory {
            return Err(Errno::ESTALE);
        }
        let inode = sb.iget(node);
        let dentry = match inode.any_alias() {
            Some(dentry) => dentry,
            None => {
                let node = inode.node().clone();
                let linked = node.getattr().await?.nlink > 0;
                Dentry::disconnected(sb, node, linked)
            }
        };
        if let Some(cred) = check {
            acceptable(cred, &dentry, &root.dentry, check_subtree).await?;
        }
        Ok(Path {
            mnt: root.mnt.clone(),
            dentry,
        })
    }
}

/// fs/fhandle.c: vfs_dentry_acceptable. Check mapped ids along the ancestry; the
/// mount-namespace-owner variant must reach root, including for the root itself.
async fn acceptable(
    cred: &Cred,
    dentry: &Arc<Dentry>,
    root: &Arc<Dentry>,
    subtree: bool,
) -> FsResult<()> {
    let mut at = dentry.clone();
    loop {
        let attr = at.node().getattr().await?;
        if !cred.user_ns.has_uid(attr.uid) || !cred.user_ns.has_gid(attr.gid) {
            return Err(Errno::ESTALE);
        }
        if Arc::ptr_eq(&at, root) {
            return Ok(());
        }
        match at.parent() {
            Some(parent) => at = parent,
            None => return if subtree { Err(Errno::ESTALE) } else { Ok(()) },
        }
    }
}
