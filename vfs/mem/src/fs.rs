use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock, Weak};

use vfs_core::provider::MountFlags;
use vfs_core::traits_sync::FsSync;
use vfs_core::{BackendInodeId, VfsCapabilities, VfsError, VfsErrorKind, VfsResult};

use crate::config::MemFsConfig;
use crate::inode::{MemInode, MemInodeKind};
use crate::node::MemNode;

#[derive(Debug)]
pub(crate) struct MemFsInner {
    next_inode: AtomicU64,
    inode_count: AtomicU64,
    used_bytes: AtomicU64,
    pub(crate) root: Arc<MemInode>,
    inode_table: RwLock<HashMap<BackendInodeId, Weak<MemInode>>>,
    pub(crate) mount_flags: MountFlags,
    pub(crate) config: MemFsConfig,
}

impl MemFsInner {
    pub(crate) fn check_writable(&self) -> VfsResult<()> {
        if self.mount_flags.contains(MountFlags::READ_ONLY) {
            return Err(VfsError::new(VfsErrorKind::ReadOnlyFs, "memfs.read_only"));
        }
        Ok(())
    }

    pub(crate) fn alloc_inode(
        fs: &Arc<MemFsInner>,
        kind: MemInodeKind,
    ) -> VfsResult<Arc<MemInode>> {
        let mut current = fs.inode_count.load(Ordering::Acquire);
        if let Some(limit) = fs.config.max_inodes {
            loop {
                if current >= limit {
                    return Err(VfsError::new(VfsErrorKind::NoSpace, "memfs.max_inodes"));
                }
                match fs.inode_count.compare_exchange(
                    current,
                    current + 1,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                ) {
                    Ok(_) => break,
                    Err(next) => current = next,
                }
            }
        } else {
            fs.inode_count.fetch_add(1, Ordering::AcqRel);
        }

        let raw = fs.next_inode.fetch_add(1, Ordering::Relaxed);
        let inode_id =
            BackendInodeId::new(raw).ok_or(VfsError::new(VfsErrorKind::Internal, "memfs.inode"))?;
        let inode = MemInode::new_child(Arc::downgrade(fs), inode_id, kind);
        let mut table = fs
            .inode_table
            .write()
            .map_err(|_| VfsError::new(VfsErrorKind::Internal, "memfs.inode_table"))?;
        table.insert(inode_id, Arc::downgrade(&inode));
        Ok(inode)
    }

    pub(crate) fn note_inode_drop(&self, inode: BackendInodeId, bytes: u64) {
        if bytes > 0 {
            self.used_bytes.fetch_sub(bytes, Ordering::AcqRel);
        }
        self.inode_count.fetch_sub(1, Ordering::AcqRel);
        if let Ok(mut table) = self.inode_table.write() {
            table.remove(&inode);
        }
    }

    pub(crate) fn try_reserve_bytes(&self, delta: u64) -> VfsResult<()> {
        if delta == 0 {
            return Ok(());
        }
        if let Some(limit) = self.config.max_bytes {
            let mut current = self.used_bytes.load(Ordering::Acquire);
            loop {
                let next = current
                    .checked_add(delta)
                    .ok_or(VfsError::new(VfsErrorKind::Overflow, "memfs.bytes"))?;
                if next > limit {
                    return Err(VfsError::new(VfsErrorKind::NoSpace, "memfs.max_bytes"));
                }
                match self.used_bytes.compare_exchange(
                    current,
                    next,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                ) {
                    Ok(_) => break,
                    Err(found) => current = found,
                }
            }
            Ok(())
        } else {
            self.used_bytes.fetch_add(delta, Ordering::AcqRel);
            Ok(())
        }
    }

    pub(crate) fn release_bytes(&self, delta: u64) {
        if delta > 0 {
            self.used_bytes.fetch_sub(delta, Ordering::AcqRel);
        }
    }
}

#[derive(Debug)]
pub struct MemFs {
    pub(crate) inner: Arc<MemFsInner>,
}

impl Default for MemFs {
    fn default() -> Self {
        Self::new()
    }
}

impl MemFs {
    pub fn new() -> Self {
        Self::new_with(MemFsConfig::default(), MountFlags::empty())
    }

    pub fn new_with(config: MemFsConfig, mount_flags: MountFlags) -> Self {
        let inner = Arc::new_cyclic(|weak| {
            let root = MemInode::new_root(weak.clone());
            let mut table = HashMap::new();
            table.insert(root.id(), Arc::downgrade(&root));
            MemFsInner {
                next_inode: AtomicU64::new(2),
                inode_count: AtomicU64::new(1),
                used_bytes: AtomicU64::new(0),
                root,
                inode_table: RwLock::new(table),
                mount_flags,
                config,
            }
        });
        Self { inner }
    }
}

impl FsSync for MemFs {
    fn provider_name(&self) -> &'static str {
        "mem"
    }

    fn capabilities(&self) -> VfsCapabilities {
        VfsCapabilities::SYMLINKS.union(VfsCapabilities::HARDLINKS)
    }

    fn root(&self) -> Arc<dyn vfs_core::traits_sync::FsNodeSync> {
        Arc::new(MemNode::new(self.inner.clone(), self.inner.root.clone()))
    }

    fn node_by_inode(
        &self,
        inode: BackendInodeId,
    ) -> Option<Arc<dyn vfs_core::traits_sync::FsNodeSync>> {
        let mut table = self.inner.inode_table.write().ok()?;
        match table.get(&inode).and_then(|weak| weak.upgrade()) {
            Some(inode) => Some(Arc::new(MemNode::new(self.inner.clone(), inode))),
            None => {
                table.remove(&inode);
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inode::MemInodeKind;
    use crate::node::MemNode;
    use vfs_core::VfsErrorKind;
    use vfs_core::flags::{OpenFlags, OpenOptions, ResolveFlags};
    use vfs_core::node::{CreateFile, MkdirOptions, RenameOptions, SetMetadata, UnlinkOptions};
    use vfs_core::path_types::{VfsName, VfsPath};
    use vfs_core::traits_sync::FsNodeSync;

    fn name(bytes: &[u8]) -> VfsName<'_> {
        VfsName::new(bytes).expect("name")
    }

    fn force_create_file(fs: &Arc<MemFsInner>, name: &[u8]) -> Arc<MemInode> {
        let inode = MemFsInner::alloc_inode(
            fs,
            MemInodeKind::File {
                data: RwLock::new(Vec::new()),
            },
        )
        .expect("inode");
        let mut children = fs.root.dir_children().expect("dir").write().expect("lock");
        children.insert(name.to_vec(), inode.clone());
        inode
    }

    #[test]
    fn read_only_mount_blocks_mutations() {
        let fs = MemFs::new_with(MemFsConfig::default(), MountFlags::READ_ONLY);
        let root = MemNode::new(fs.inner.clone(), fs.inner.root.clone());

        let err = root.create_file(&name(b"file"), CreateFile::default());
        match err {
            Ok(_) => panic!("expected error"),
            Err(err) => assert_eq!(err.kind(), VfsErrorKind::ReadOnlyFs),
        }

        let err = root.mkdir(&name(b"dir"), MkdirOptions::default());
        match err {
            Ok(_) => panic!("expected error"),
            Err(err) => assert_eq!(err.kind(), VfsErrorKind::ReadOnlyFs),
        }

        let err = root
            .unlink(&name(b"missing"), UnlinkOptions { must_be_dir: false })
            .unwrap_err();
        assert_eq!(err.kind(), VfsErrorKind::ReadOnlyFs);

        let err = root.rmdir(&name(b"missing")).unwrap_err();
        assert_eq!(err.kind(), VfsErrorKind::ReadOnlyFs);

        let err = root
            .rename(&name(b"a"), &root, &name(b"b"), RenameOptions::default())
            .unwrap_err();
        assert_eq!(err.kind(), VfsErrorKind::ReadOnlyFs);

        let err = root
            .symlink(&name(b"link"), VfsPath::new(b"/"))
            .unwrap_err();
        assert_eq!(err.kind(), VfsErrorKind::ReadOnlyFs);

        let inode = force_create_file(&fs.inner, b"existing");
        let node = MemNode::new(fs.inner.clone(), inode);
        let handle = node
            .open(OpenOptions {
                flags: OpenFlags::READ | OpenFlags::WRITE,
                mode: None,
                resolve: ResolveFlags::empty(),
            })
            .expect("open");

        let err = node
            .set_metadata(SetMetadata {
                size: Some(1),
                ..SetMetadata::default()
            })
            .unwrap_err();
        assert_eq!(err.kind(), VfsErrorKind::ReadOnlyFs);

        let err = handle.write_at(0, b"data").unwrap_err();
        assert_eq!(err.kind(), VfsErrorKind::ReadOnlyFs);

        let err = handle.set_len(2).unwrap_err();
        assert_eq!(err.kind(), VfsErrorKind::ReadOnlyFs);
    }
}
