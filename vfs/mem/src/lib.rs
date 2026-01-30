use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock, Weak};

use vfs_core::dir::VfsDirEntry;
use vfs_core::flags::{OpenFlags, OpenOptions};
use vfs_core::node::{
    CreateFile, DirCursor, FsHandle, FsNode, MkdirOptions, ReadDirBatch, RenameOptions,
    SetMetadata, UnlinkOptions, VfsDirCookie,
};
use vfs_core::path_types::{VfsName, VfsNameBuf, VfsPath, VfsPathBuf};
use vfs_core::{
    BackendInodeId, Fs, MountId, VfsCapabilities, VfsError, VfsErrorKind, VfsFileMode, VfsFileType,
    VfsInodeId, VfsMetadata, VfsResult, VfsTimespec,
};

#[derive(Debug)]
struct MemFsInner {
    next_inode: AtomicU64,
    root: Arc<MemNode>,
}

impl MemFsInner {
    fn next_inode(&self) -> BackendInodeId {
        let raw = self.next_inode.fetch_add(1, Ordering::Relaxed);
        BackendInodeId::new(raw).expect("memfs inode must be non-zero")
    }
}

#[derive(Debug)]
pub struct MemFs {
    inner: Arc<MemFsInner>,
}

impl Default for MemFs {
    fn default() -> Self {
        Self::new()
    }
}

impl MemFs {
    pub fn new() -> Self {
        let inner = Arc::new_cyclic(|weak| {
            let fs: Arc<MemFsInner> = weak.upgrade().expect("memfs upgrade");
            let root = MemNode::new_root(fs.clone());
            MemFsInner {
                next_inode: AtomicU64::new(2),
                root,
            }
        });
        Self { inner }
    }
}

impl Fs for MemFs {
    fn provider_name(&self) -> &'static str {
        "mem"
    }

    fn capabilities(&self) -> VfsCapabilities {
        VfsCapabilities::NONE
    }

    fn root(&self) -> Arc<dyn FsNode> {
        self.inner.root.clone()
    }
}

#[derive(Debug, Clone)]
struct MemMetadata {
    mode: VfsFileMode,
    uid: u32,
    gid: u32,
    atime: VfsTimespec,
    mtime: VfsTimespec,
    ctime: VfsTimespec,
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
enum MemNodeKind {
    File {
        data: RwLock<Vec<u8>>,
    },
    Dir {
        children: RwLock<BTreeMap<Vec<u8>, Arc<MemNode>>>,
    },
    Symlink {
        target: VfsPathBuf,
    },
}

#[derive(Debug)]
struct MemNode {
    self_ref: Weak<MemNode>,
    fs: Arc<MemFsInner>,
    inode: BackendInodeId,
    kind: MemNodeKind,
    meta: RwLock<MemMetadata>,
}

impl MemNode {
    fn new_root(fs: Arc<MemFsInner>) -> Arc<MemNode> {
        Arc::new_cyclic(|weak| MemNode {
            self_ref: weak.clone(),
            fs,
            inode: BackendInodeId::new(1).expect("root inode"),
            kind: MemNodeKind::Dir {
                children: RwLock::new(BTreeMap::new()),
            },
            meta: RwLock::new(MemMetadata::default()),
        })
    }

    fn new_child(fs: Arc<MemFsInner>, kind: MemNodeKind) -> Arc<MemNode> {
        let inode = fs.next_inode();
        Arc::new_cyclic(|weak| MemNode {
            self_ref: weak.clone(),
            fs,
            inode,
            kind,
            meta: RwLock::new(MemMetadata::default()),
        })
    }

    fn file_type(&self) -> VfsFileType {
        match self.kind {
            MemNodeKind::File { .. } => VfsFileType::RegularFile,
            MemNodeKind::Dir { .. } => VfsFileType::Directory,
            MemNodeKind::Symlink { .. } => VfsFileType::Symlink,
        }
    }

    fn dir_children(&self) -> VfsResult<&RwLock<BTreeMap<Vec<u8>, Arc<MemNode>>>> {
        match &self.kind {
            MemNodeKind::Dir { children } => Ok(children),
            _ => Err(VfsError::new(VfsErrorKind::NotDir, "memfs.dir_children")),
        }
    }

    fn ensure_dir(&self) -> VfsResult<()> {
        if matches!(self.kind, MemNodeKind::Dir { .. }) {
            Ok(())
        } else {
            Err(VfsError::new(VfsErrorKind::NotDir, "memfs.ensure_dir"))
        }
    }

    fn empty_dir(children: &BTreeMap<Vec<u8>, Arc<MemNode>>) -> bool {
        children.is_empty()
    }

    fn arc_self(&self) -> VfsResult<Arc<MemNode>> {
        self.self_ref
            .upgrade()
            .ok_or(VfsError::new(VfsErrorKind::Internal, "memfs.arc_self"))
    }
}

impl FsNode for MemNode {
    fn inode(&self) -> BackendInodeId {
        self.inode
    }

    fn file_type(&self) -> VfsFileType {
        self.file_type()
    }

    fn metadata(&self) -> VfsResult<VfsMetadata> {
        let meta = self.meta.read().expect("lock");
        let size = match &self.kind {
            MemNodeKind::File { data } => data.read().expect("lock").len() as u64,
            _ => 0,
        };
        Ok(VfsMetadata {
            inode: VfsInodeId {
                mount: MountId::from_index(0),
                backend: self.inode,
            },
            file_type: self.file_type(),
            mode: meta.mode,
            uid: meta.uid,
            gid: meta.gid,
            nlink: 1,
            size,
            atime: meta.atime,
            mtime: meta.mtime,
            ctime: meta.ctime,
            rdev_major: 0,
            rdev_minor: 0,
        })
    }

    fn set_metadata(&self, set: SetMetadata) -> VfsResult<()> {
        {
            let mut meta = self.meta.write().expect("lock");
            if let Some(mode) = set.mode {
                meta.mode = mode;
            }
            if let Some(uid) = set.uid {
                meta.uid = uid;
            }
            if let Some(gid) = set.gid {
                meta.gid = gid;
            }
            if let Some(atime) = set.atime {
                meta.atime = atime;
            }
            if let Some(mtime) = set.mtime {
                meta.mtime = mtime;
            }
            if let Some(ctime) = set.ctime {
                meta.ctime = ctime;
            }
        }

        if let Some(size) = set.size {
            match &self.kind {
                MemNodeKind::File { data } => {
                    let mut data = data.write().expect("lock");
                    let len = size as usize;
                    if data.len() > len {
                        data.truncate(len);
                    } else {
                        data.resize(len, 0);
                    }
                    Ok(())
                }
                _ => Err(VfsError::new(
                    VfsErrorKind::InvalidInput,
                    "memfs.set_metadata.size",
                )),
            }
        } else {
            Ok(())
        }
    }

    fn lookup(&self, name: &VfsName) -> VfsResult<Arc<dyn FsNode>> {
        self.ensure_dir()?;
        let children = self.dir_children()?.read().expect("lock");
        let node = children
            .get(name.as_bytes())
            .ok_or(VfsError::new(VfsErrorKind::NotFound, "memfs.lookup"))?;
        Ok(node.clone())
    }

    fn create_file(&self, name: &VfsName, opts: CreateFile) -> VfsResult<Arc<dyn FsNode>> {
        self.ensure_dir()?;
        let mut children = self.dir_children()?.write().expect("lock");
        if let Some(existing) = children.get(name.as_bytes()) {
            if existing.file_type() == VfsFileType::Directory {
                return Err(VfsError::new(VfsErrorKind::IsDir, "memfs.create_file"));
            }
            if opts.exclusive {
                return Err(VfsError::new(
                    VfsErrorKind::AlreadyExists,
                    "memfs.create_file",
                ));
            }
            if opts.truncate {
                if let MemNodeKind::File { data } = &existing.kind {
                    data.write().expect("lock").clear();
                }
            }
            return Ok(existing.clone());
        }

        let node = MemNode::new_child(
            self.fs.clone(),
            MemNodeKind::File {
                data: RwLock::new(Vec::new()),
            },
        );
        if let Some(mode) = opts.mode {
            node.meta.write().expect("lock").mode = VfsFileMode(mode);
        }
        children.insert(name.as_bytes().to_vec(), node.clone());
        Ok(node)
    }

    fn mkdir(&self, name: &VfsName, opts: MkdirOptions) -> VfsResult<Arc<dyn FsNode>> {
        self.ensure_dir()?;
        let mut children = self.dir_children()?.write().expect("lock");
        if children.contains_key(name.as_bytes()) {
            return Err(VfsError::new(VfsErrorKind::AlreadyExists, "memfs.mkdir"));
        }
        let node = MemNode::new_child(
            self.fs.clone(),
            MemNodeKind::Dir {
                children: RwLock::new(BTreeMap::new()),
            },
        );
        if let Some(mode) = opts.mode {
            node.meta.write().expect("lock").mode = VfsFileMode(mode);
        }
        children.insert(name.as_bytes().to_vec(), node.clone());
        Ok(node)
    }

    fn unlink(&self, name: &VfsName, opts: UnlinkOptions) -> VfsResult<()> {
        self.ensure_dir()?;
        let mut children = self.dir_children()?.write().expect("lock");
        let entry = children
            .get(name.as_bytes())
            .ok_or(VfsError::new(VfsErrorKind::NotFound, "memfs.unlink"))?;
        if opts.must_be_dir && entry.file_type() != VfsFileType::Directory {
            return Err(VfsError::new(VfsErrorKind::NotDir, "memfs.unlink"));
        }
        if !opts.must_be_dir && entry.file_type() == VfsFileType::Directory {
            return Err(VfsError::new(VfsErrorKind::IsDir, "memfs.unlink"));
        }
        children.remove(name.as_bytes());
        Ok(())
    }

    fn rmdir(&self, name: &VfsName) -> VfsResult<()> {
        self.ensure_dir()?;
        let mut children = self.dir_children()?.write().expect("lock");
        let entry = children
            .get(name.as_bytes())
            .ok_or(VfsError::new(VfsErrorKind::NotFound, "memfs.rmdir"))?;
        if entry.file_type() != VfsFileType::Directory {
            return Err(VfsError::new(VfsErrorKind::NotDir, "memfs.rmdir"));
        }
        if let MemNodeKind::Dir { children: inner } = &entry.kind {
            if !MemNode::empty_dir(&inner.read().expect("lock")) {
                return Err(VfsError::new(VfsErrorKind::DirNotEmpty, "memfs.rmdir"));
            }
        }
        children.remove(name.as_bytes());
        Ok(())
    }

    fn read_dir(&self, cursor: Option<DirCursor>, max: usize) -> VfsResult<ReadDirBatch> {
        self.ensure_dir()?;
        let children = self.dir_children()?.read().expect("lock");
        let start = cursor.map(|c| c.0 as usize).unwrap_or(0);
        let mut entries = Vec::new();
        for (name, node) in children.iter() {
            entries.push(VfsDirEntry {
                name: VfsNameBuf::new(name.clone())?,
                inode: Some(node.inode()),
                file_type: Some(node.file_type()),
            });
        }
        if start >= entries.len() {
            return Ok(ReadDirBatch {
                entries: Default::default(),
                next: None,
            });
        }
        let end = entries.len().min(start + max);
        let mut batch = ReadDirBatch {
            entries: entries[start..end].iter().cloned().collect(),
            next: None,
        };
        if end < entries.len() {
            batch.next = Some(VfsDirCookie(end as u64));
        }
        Ok(batch)
    }

    fn rename(
        &self,
        old_name: &VfsName,
        new_parent: &dyn FsNode,
        new_name: &VfsName,
        opts: RenameOptions,
    ) -> VfsResult<()> {
        if opts.exchange {
            return Err(VfsError::new(
                VfsErrorKind::NotSupported,
                "memfs.rename.exchange",
            ));
        }
        let new_parent = new_parent
            .as_any()
            .downcast_ref::<MemNode>()
            .ok_or(VfsError::new(VfsErrorKind::CrossDevice, "memfs.rename"))?;
        self.ensure_dir()?;
        new_parent.ensure_dir()?;

        let mut src_children = self.dir_children()?.write().expect("lock");
        let node = src_children
            .remove(old_name.as_bytes())
            .ok_or(VfsError::new(VfsErrorKind::NotFound, "memfs.rename"))?;

        let mut dst_children = new_parent.dir_children()?.write().expect("lock");
        if dst_children.contains_key(new_name.as_bytes()) {
            if opts.noreplace {
                src_children.insert(old_name.as_bytes().to_vec(), node);
                return Err(VfsError::new(VfsErrorKind::AlreadyExists, "memfs.rename"));
            }
            let existing = dst_children.remove(new_name.as_bytes()).expect("exists");
            if existing.file_type() == VfsFileType::Directory {
                if let MemNodeKind::Dir { children } = &existing.kind {
                    if !MemNode::empty_dir(&children.read().expect("lock")) {
                        src_children.insert(old_name.as_bytes().to_vec(), node);
                        dst_children.insert(new_name.as_bytes().to_vec(), existing);
                        return Err(VfsError::new(VfsErrorKind::DirNotEmpty, "memfs.rename"));
                    }
                }
            }
        }
        dst_children.insert(new_name.as_bytes().to_vec(), node);
        Ok(())
    }

    fn link(&self, _existing: &dyn FsNode, _new_name: &VfsName) -> VfsResult<()> {
        Err(VfsError::new(VfsErrorKind::NotSupported, "memfs.link"))
    }

    fn symlink(&self, new_name: &VfsName, target: &VfsPath) -> VfsResult<()> {
        self.ensure_dir()?;
        let mut children = self.dir_children()?.write().expect("lock");
        if children.contains_key(new_name.as_bytes()) {
            return Err(VfsError::new(VfsErrorKind::AlreadyExists, "memfs.symlink"));
        }
        let node = MemNode::new_child(
            self.fs.clone(),
            MemNodeKind::Symlink {
                target: target.to_path_buf(),
            },
        );
        children.insert(new_name.as_bytes().to_vec(), node);
        Ok(())
    }

    fn readlink(&self) -> VfsResult<VfsPathBuf> {
        match &self.kind {
            MemNodeKind::Symlink { target } => Ok(target.clone()),
            _ => Err(VfsError::new(VfsErrorKind::InvalidInput, "memfs.readlink")),
        }
    }

    fn open(&self, opts: OpenOptions) -> VfsResult<Arc<dyn FsHandle>> {
        if self.file_type() != VfsFileType::RegularFile {
            return Err(VfsError::new(VfsErrorKind::IsDir, "memfs.open"));
        }
        let node = self.arc_self()?;
        Ok(Arc::new(MemHandle {
            node,
            flags: opts.flags,
        }))
    }
}

#[derive(Debug, Clone)]
struct MemHandle {
    node: Arc<MemNode>,
    flags: OpenFlags,
}

impl MemHandle {
    fn can_write(&self) -> bool {
        self.flags.contains(OpenFlags::WRITE) || self.flags.contains(OpenFlags::APPEND)
    }
}

impl FsHandle for MemHandle {
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> VfsResult<usize> {
        let data = match &self.node.kind {
            MemNodeKind::File { data } => data.read().expect("lock"),
            _ => {
                return Err(VfsError::new(VfsErrorKind::InvalidInput, "memfs.read_at"));
            }
        };
        let offset = offset as usize;
        if offset >= data.len() {
            return Ok(0);
        }
        let end = (offset + buf.len()).min(data.len());
        buf[..end - offset].copy_from_slice(&data[offset..end]);
        Ok(end - offset)
    }

    fn write_at(&self, offset: u64, buf: &[u8]) -> VfsResult<usize> {
        if !self.can_write() {
            return Err(VfsError::new(
                VfsErrorKind::PermissionDenied,
                "memfs.write_at",
            ));
        }
        let data = match &self.node.kind {
            MemNodeKind::File { data } => data,
            _ => {
                return Err(VfsError::new(VfsErrorKind::InvalidInput, "memfs.write_at"));
            }
        };
        let mut data = data.write().expect("lock");
        let offset = offset as usize;
        if offset > data.len() {
            data.resize(offset, 0);
        }
        if offset + buf.len() > data.len() {
            data.resize(offset + buf.len(), 0);
        }
        data[offset..offset + buf.len()].copy_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&self) -> VfsResult<()> {
        Ok(())
    }

    fn fsync(&self) -> VfsResult<()> {
        Ok(())
    }

    fn set_len(&self, len: u64) -> VfsResult<()> {
        let data = match &self.node.kind {
            MemNodeKind::File { data } => data,
            _ => {
                return Err(VfsError::new(VfsErrorKind::InvalidInput, "memfs.set_len"));
            }
        };
        let mut data = data.write().expect("lock");
        let len = len as usize;
        if data.len() > len {
            data.truncate(len);
        } else {
            data.resize(len, 0);
        }
        Ok(())
    }

    fn len(&self) -> VfsResult<u64> {
        let data = match &self.node.kind {
            MemNodeKind::File { data } => data.read().expect("lock"),
            _ => {
                return Err(VfsError::new(VfsErrorKind::InvalidInput, "memfs.len"));
            }
        };
        Ok(data.len() as u64)
    }

    fn dup(&self) -> VfsResult<Option<Arc<dyn FsHandle>>> {
        Ok(Some(Arc::new(self.clone())))
    }
}
