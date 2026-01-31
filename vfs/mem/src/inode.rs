use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock, Weak};

use vfs_core::path_types::VfsPathBuf;
use vfs_core::{
    BackendInodeId, MountId, VfsError, VfsErrorKind, VfsFileMode, VfsFileType, VfsInodeId,
    VfsMetadata, VfsResult, VfsTimespec,
};

use crate::fs::MemFsInner;

#[derive(Debug, Clone)]
pub(crate) struct MemMetadata {
    pub(crate) mode: VfsFileMode,
    pub(crate) uid: u32,
    pub(crate) gid: u32,
    pub(crate) atime: VfsTimespec,
    pub(crate) mtime: VfsTimespec,
    pub(crate) ctime: VfsTimespec,
}

impl Default for MemMetadata {
    fn default() -> Self {
        Self {
            mode: VfsFileMode(0),
            uid: 0,
            gid: 0,
            atime: VfsTimespec { secs: 0, nanos: 0 },
            mtime: VfsTimespec { secs: 0, nanos: 0 },
            ctime: VfsTimespec { secs: 0, nanos: 0 },
        }
    }
}

#[derive(Debug)]
pub(crate) enum MemInodeKind {
    File {
        data: RwLock<Vec<u8>>,
    },
    Dir {
        children: RwLock<BTreeMap<Vec<u8>, Arc<MemInode>>>,
    },
    Symlink {
        target: VfsPathBuf,
    },
}

#[derive(Debug)]
pub(crate) struct MemInode {
    id: BackendInodeId,
    pub(crate) kind: MemInodeKind,
    pub(crate) meta: RwLock<MemMetadata>,
    nlink: AtomicU64,
    fs: Weak<MemFsInner>,
}

impl MemInode {
    pub(crate) fn new_root(fs: Weak<MemFsInner>) -> Arc<Self> {
        Arc::new(Self {
            id: BackendInodeId::new(1).expect("memfs root inode"),
            kind: MemInodeKind::Dir {
                children: RwLock::new(BTreeMap::new()),
            },
            meta: RwLock::new(MemMetadata::default()),
            nlink: AtomicU64::new(1),
            fs,
        })
    }

    pub(crate) fn new_child(
        fs: Weak<MemFsInner>,
        id: BackendInodeId,
        kind: MemInodeKind,
    ) -> Arc<Self> {
        Arc::new(Self {
            id,
            kind,
            meta: RwLock::new(MemMetadata::default()),
            nlink: AtomicU64::new(1),
            fs,
        })
    }

    pub(crate) fn id(&self) -> BackendInodeId {
        self.id
    }

    pub(crate) fn file_type(&self) -> VfsFileType {
        match self.kind {
            MemInodeKind::File { .. } => VfsFileType::RegularFile,
            MemInodeKind::Dir { .. } => VfsFileType::Directory,
            MemInodeKind::Symlink { .. } => VfsFileType::Symlink,
        }
    }

    pub(crate) fn dir_children(&self) -> VfsResult<&RwLock<BTreeMap<Vec<u8>, Arc<MemInode>>>> {
        match &self.kind {
            MemInodeKind::Dir { children } => Ok(children),
            _ => Err(VfsError::new(VfsErrorKind::NotDir, "memfs.dir_children")),
        }
    }

    pub(crate) fn file_data(&self) -> VfsResult<&RwLock<Vec<u8>>> {
        match &self.kind {
            MemInodeKind::File { data } => Ok(data),
            _ => Err(VfsError::new(VfsErrorKind::InvalidInput, "memfs.file_data")),
        }
    }

    pub(crate) fn nlink(&self) -> u64 {
        if self.file_type() == VfsFileType::RegularFile {
            self.nlink.load(Ordering::Acquire)
        } else {
            1
        }
    }

    pub(crate) fn inc_nlink(&self) -> VfsResult<()> {
        let current = self.nlink.fetch_add(1, Ordering::AcqRel);
        if current == u64::MAX {
            self.nlink.fetch_sub(1, Ordering::AcqRel);
            return Err(VfsError::new(VfsErrorKind::TooManyLinks, "memfs.link"));
        }
        Ok(())
    }

    pub(crate) fn dec_nlink(&self) {
        let current = self.nlink.load(Ordering::Acquire);
        if current == 0 {
            return;
        }
        self.nlink.fetch_sub(1, Ordering::AcqRel);
    }

    pub(crate) fn metadata(&self) -> VfsMetadata {
        let meta = self.meta.read().expect("lock");
        let size = match &self.kind {
            MemInodeKind::File { data } => data.read().expect("lock").len() as u64,
            MemInodeKind::Symlink { target } => target.as_bytes().len() as u64,
            _ => 0,
        };
        VfsMetadata {
            inode: VfsInodeId {
                mount: MountId::from_index(0),
                backend: self.id,
            },
            file_type: self.file_type(),
            mode: meta.mode,
            uid: meta.uid,
            gid: meta.gid,
            nlink: self.nlink(),
            size,
            atime: meta.atime,
            mtime: meta.mtime,
            ctime: meta.ctime,
            rdev_major: 0,
            rdev_minor: 0,
        }
    }
}

impl Drop for MemInode {
    fn drop(&mut self) {
        let fs = match self.fs.upgrade() {
            Some(fs) => fs,
            None => return,
        };
        let bytes = match &self.kind {
            MemInodeKind::File { data } => data.read().expect("lock").len() as u64,
            _ => 0,
        };
        fs.note_inode_drop(self.id, bytes);
    }
}
