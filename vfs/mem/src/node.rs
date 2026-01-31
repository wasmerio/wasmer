use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use vfs_core::dir::VfsDirEntry;
use vfs_core::flags::OpenOptions;
use vfs_core::node::{
    CreateFile, DirCursor, FsHandle, FsNode, MkdirOptions, ReadDirBatch, RenameOptions,
    SetMetadata, UnlinkOptions, VfsDirCookie,
};
use vfs_core::path_types::{VfsName, VfsNameBuf, VfsPath, VfsPathBuf};
use vfs_core::{VfsError, VfsErrorKind, VfsFileMode, VfsFileType, VfsResult};

use crate::fs::MemFsInner;
use crate::handle::MemHandle;
use crate::inode::{MemInode, MemInodeKind};

#[derive(Debug)]
pub(crate) struct MemNode {
    fs: Arc<MemFsInner>,
    inode: Arc<MemInode>,
}

impl MemNode {
    pub(crate) fn new(fs: Arc<MemFsInner>, inode: Arc<MemInode>) -> Self {
        Self { fs, inode }
    }

    fn ensure_dir(&self) -> VfsResult<()> {
        if matches!(self.inode.kind, MemInodeKind::Dir { .. }) {
            Ok(())
        } else {
            Err(VfsError::new(VfsErrorKind::NotDir, "memfs.ensure_dir"))
        }
    }

    fn dir_children(&self) -> VfsResult<&RwLock<BTreeMap<Vec<u8>, Arc<MemInode>>>> {
        self.inode.dir_children()
    }

    fn empty_dir(children: &BTreeMap<Vec<u8>, Arc<MemInode>>) -> bool {
        children.is_empty()
    }

    fn read_only_if_set(&self, set: &SetMetadata) -> VfsResult<()> {
        if !self
            .fs
            .mount_flags
            .contains(vfs_core::provider::MountFlags::READ_ONLY)
        {
            return Ok(());
        }
        let has_change = set.mode.is_some()
            || set.uid.is_some()
            || set.gid.is_some()
            || set.size.is_some()
            || set.atime.is_some()
            || set.mtime.is_some()
            || set.ctime.is_some();
        if has_change {
            return Err(VfsError::new(VfsErrorKind::ReadOnlyFs, "memfs.read_only"));
        }
        Ok(())
    }

    fn resize_file(&self, inode: &MemInode, new_len: u64) -> VfsResult<()> {
        let data = inode.file_data()?;
        let mut data = data.write().expect("lock");
        let old_len = data.len();
        let new_len_usize = new_len as usize;
        if new_len_usize > old_len {
            let delta = (new_len_usize - old_len) as u64;
            self.fs.try_reserve_bytes(delta)?;
            data.resize(new_len_usize, 0);
        } else if new_len_usize < old_len {
            let delta = (old_len - new_len_usize) as u64;
            data.truncate(new_len_usize);
            self.fs.release_bytes(delta);
        }
        Ok(())
    }
}

impl FsNode for MemNode {
    fn inode(&self) -> vfs_core::BackendInodeId {
        self.inode.id()
    }

    fn file_type(&self) -> VfsFileType {
        self.inode.file_type()
    }

    fn metadata(&self) -> VfsResult<vfs_core::VfsMetadata> {
        Ok(self.inode.metadata())
    }

    fn set_metadata(&self, set: SetMetadata) -> VfsResult<()> {
        self.read_only_if_set(&set)?;
        {
            let mut meta = self.inode.meta.write().expect("lock");
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
            match self.inode.file_type() {
                VfsFileType::RegularFile => self.resize_file(&self.inode, size),
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
        let inode = children
            .get(name.as_bytes())
            .ok_or(VfsError::new(VfsErrorKind::NotFound, "memfs.lookup"))?;
        Ok(Arc::new(MemNode::new(self.fs.clone(), inode.clone())))
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
                self.fs.check_writable()?;
                self.resize_file(existing, 0)?;
            }
            return Ok(Arc::new(MemNode::new(self.fs.clone(), existing.clone())));
        }

        self.fs.check_writable()?;
        let inode = MemFsInner::alloc_inode(
            &self.fs,
            MemInodeKind::File {
                data: RwLock::new(Vec::new()),
            },
        )?;
        if let Some(mode) = opts.mode {
            inode.meta.write().expect("lock").mode = VfsFileMode(mode);
        }
        children.insert(name.as_bytes().to_vec(), inode.clone());
        Ok(Arc::new(MemNode::new(self.fs.clone(), inode)))
    }

    fn mkdir(&self, name: &VfsName, opts: MkdirOptions) -> VfsResult<Arc<dyn FsNode>> {
        self.ensure_dir()?;
        self.fs.check_writable()?;
        let mut children = self.dir_children()?.write().expect("lock");
        if children.contains_key(name.as_bytes()) {
            return Err(VfsError::new(VfsErrorKind::AlreadyExists, "memfs.mkdir"));
        }
        let inode = MemFsInner::alloc_inode(
            &self.fs,
            MemInodeKind::Dir {
                children: RwLock::new(BTreeMap::new()),
            },
        )?;
        if let Some(mode) = opts.mode {
            inode.meta.write().expect("lock").mode = VfsFileMode(mode);
        }
        children.insert(name.as_bytes().to_vec(), inode.clone());
        Ok(Arc::new(MemNode::new(self.fs.clone(), inode)))
    }

    fn unlink(&self, name: &VfsName, opts: UnlinkOptions) -> VfsResult<()> {
        self.fs.check_writable()?;
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
        let removed = children.remove(name.as_bytes()).expect("entry");
        if removed.file_type() == VfsFileType::RegularFile {
            removed.dec_nlink();
        }
        Ok(())
    }

    fn rmdir(&self, name: &VfsName) -> VfsResult<()> {
        self.fs.check_writable()?;
        self.ensure_dir()?;
        let mut children = self.dir_children()?.write().expect("lock");
        let entry = children
            .get(name.as_bytes())
            .ok_or(VfsError::new(VfsErrorKind::NotFound, "memfs.rmdir"))?;
        if entry.file_type() != VfsFileType::Directory {
            return Err(VfsError::new(VfsErrorKind::NotDir, "memfs.rmdir"));
        }
        if let MemInodeKind::Dir { children: inner } = &entry.kind {
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
        let start = match cursor {
            Some(c) => usize::try_from(c.0).unwrap_or(usize::MAX),
            None => 0,
        };
        let mut entries = Vec::new();
        for (name, inode) in children.iter() {
            entries.push(VfsDirEntry {
                name: VfsNameBuf::new(name.clone())?,
                inode: Some(inode.id()),
                file_type: Some(inode.file_type()),
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
        self.fs.check_writable()?;
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
        if !Arc::ptr_eq(&self.fs, &new_parent.fs) {
            return Err(VfsError::new(VfsErrorKind::CrossDevice, "memfs.rename"));
        }
        self.ensure_dir()?;
        new_parent.ensure_dir()?;
        if self.inode.id() == new_parent.inode.id() && old_name.as_bytes() == new_name.as_bytes() {
            return Ok(());
        }

        if self.inode.id() == new_parent.inode.id() {
            let mut children = self.dir_children()?.write().expect("lock");
            let node = children
                .remove(old_name.as_bytes())
                .ok_or(VfsError::new(VfsErrorKind::NotFound, "memfs.rename"))?;
            if let Some(existing) = children.get(new_name.as_bytes()) {
                if opts.noreplace {
                    children.insert(old_name.as_bytes().to_vec(), node);
                    return Err(VfsError::new(VfsErrorKind::AlreadyExists, "memfs.rename"));
                }
                if existing.file_type() == VfsFileType::Directory {
                    if let MemInodeKind::Dir { children: inner } = &existing.kind {
                        if !MemNode::empty_dir(&inner.read().expect("lock")) {
                            children.insert(old_name.as_bytes().to_vec(), node);
                            return Err(VfsError::new(VfsErrorKind::DirNotEmpty, "memfs.rename"));
                        }
                    }
                }
                let removed = children.remove(new_name.as_bytes()).expect("existing");
                if removed.file_type() == VfsFileType::RegularFile {
                    removed.dec_nlink();
                }
            }
            children.insert(new_name.as_bytes().to_vec(), node);
            return Ok(());
        }

        let left = self.dir_children()?;
        let right = new_parent.dir_children()?;
        let left_ptr = left as *const _ as usize;
        let right_ptr = right as *const _ as usize;
        if left_ptr == right_ptr {
            let mut children = left.write().expect("lock");
            let node = children
                .remove(old_name.as_bytes())
                .ok_or(VfsError::new(VfsErrorKind::NotFound, "memfs.rename"))?;
            if let Some(existing) = children.get(new_name.as_bytes()) {
                if opts.noreplace {
                    children.insert(old_name.as_bytes().to_vec(), node);
                    return Err(VfsError::new(VfsErrorKind::AlreadyExists, "memfs.rename"));
                }
                if existing.file_type() == VfsFileType::Directory {
                    if let MemInodeKind::Dir { children: inner } = &existing.kind {
                        if !MemNode::empty_dir(&inner.read().expect("lock")) {
                            children.insert(old_name.as_bytes().to_vec(), node);
                            return Err(VfsError::new(VfsErrorKind::DirNotEmpty, "memfs.rename"));
                        }
                    }
                }
                let removed = children.remove(new_name.as_bytes()).expect("existing");
                if removed.file_type() == VfsFileType::RegularFile {
                    removed.dec_nlink();
                }
            }
            children.insert(new_name.as_bytes().to_vec(), node);
            return Ok(());
        }

        let (mut src_children, mut dst_children) = match left_ptr.cmp(&right_ptr) {
            Ordering::Less => {
                let left_guard = left.write().expect("lock");
                let right_guard = right.write().expect("lock");
                (left_guard, right_guard)
            }
            Ordering::Greater => {
                let right_guard = right.write().expect("lock");
                let left_guard = left.write().expect("lock");
                (left_guard, right_guard)
            }
            Ordering::Equal => unreachable!("distinct dirs should not share lock"),
        };

        let node = src_children
            .remove(old_name.as_bytes())
            .ok_or(VfsError::new(VfsErrorKind::NotFound, "memfs.rename"))?;
        if let Some(existing) = dst_children.get(new_name.as_bytes()) {
            if opts.noreplace {
                src_children.insert(old_name.as_bytes().to_vec(), node);
                return Err(VfsError::new(VfsErrorKind::AlreadyExists, "memfs.rename"));
            }
            if existing.file_type() == VfsFileType::Directory {
                if let MemInodeKind::Dir { children } = &existing.kind {
                    if !MemNode::empty_dir(&children.read().expect("lock")) {
                        src_children.insert(old_name.as_bytes().to_vec(), node);
                        return Err(VfsError::new(VfsErrorKind::DirNotEmpty, "memfs.rename"));
                    }
                }
            }
            let removed = dst_children.remove(new_name.as_bytes()).expect("existing");
            if removed.file_type() == VfsFileType::RegularFile {
                removed.dec_nlink();
            }
        }
        dst_children.insert(new_name.as_bytes().to_vec(), node);
        Ok(())
    }

    fn link(&self, existing: &dyn FsNode, new_name: &VfsName) -> VfsResult<()> {
        self.fs.check_writable()?;
        self.ensure_dir()?;
        let existing = existing
            .as_any()
            .downcast_ref::<MemNode>()
            .ok_or(VfsError::new(VfsErrorKind::CrossDevice, "memfs.link"))?;
        if !Arc::ptr_eq(&self.fs, &existing.fs) {
            return Err(VfsError::new(VfsErrorKind::CrossDevice, "memfs.link"));
        }
        if existing.file_type() != VfsFileType::RegularFile {
            return Err(VfsError::new(VfsErrorKind::NotSupported, "memfs.link"));
        }
        let mut children = self.dir_children()?.write().expect("lock");
        if children.contains_key(new_name.as_bytes()) {
            return Err(VfsError::new(VfsErrorKind::AlreadyExists, "memfs.link"));
        }
        existing.inode.inc_nlink()?;
        children.insert(new_name.as_bytes().to_vec(), existing.inode.clone());
        Ok(())
    }

    fn symlink(&self, new_name: &VfsName, target: &VfsPath) -> VfsResult<()> {
        self.fs.check_writable()?;
        self.ensure_dir()?;
        let mut children = self.dir_children()?.write().expect("lock");
        if children.contains_key(new_name.as_bytes()) {
            return Err(VfsError::new(VfsErrorKind::AlreadyExists, "memfs.symlink"));
        }
        let inode = MemFsInner::alloc_inode(
            &self.fs,
            MemInodeKind::Symlink {
                target: target.to_path_buf(),
            },
        )?;
        children.insert(new_name.as_bytes().to_vec(), inode);
        Ok(())
    }

    fn readlink(&self) -> VfsResult<VfsPathBuf> {
        match &self.inode.kind {
            MemInodeKind::Symlink { target } => Ok(target.clone()),
            _ => Err(VfsError::new(VfsErrorKind::InvalidInput, "memfs.readlink")),
        }
    }

    fn open(&self, opts: OpenOptions) -> VfsResult<Arc<dyn FsHandle>> {
        match self.file_type() {
            VfsFileType::RegularFile => Ok(Arc::new(MemHandle::new(
                self.fs.clone(),
                self.inode.clone(),
                opts.flags,
            ))),
            VfsFileType::Directory => Err(VfsError::new(VfsErrorKind::IsDir, "memfs.open")),
            _ => Err(VfsError::new(VfsErrorKind::InvalidInput, "memfs.open")),
        }
    }
}
