
use std::sync::Arc;

use vfs_core::flags::OpenOptions;
use vfs_core::node::{
    CreateFile, DirCursor, FsHandle, FsNode, MkdirOptions, ReadDirBatch, RenameOptions,
    SetMetadata, UnlinkOptions, VfsDirCookie,
};
use vfs_core::path_types::{VfsName, VfsNameBuf, VfsPath, VfsPathBuf};
use vfs_core::{
    BackendInodeId, VfsError, VfsErrorKind, VfsFileMode, VfsFileType, VfsMetadata, VfsResult,
    VfsTimespec,
};
use webc::Metadata as WebcMetadata;

use crate::fs::WebcFs;
use crate::handle::WebcFileHandle;

#[derive(Debug)]
pub struct WebcNode {
    inner: Arc<crate::fs::WebcFsInner>,
    path: VfsPathBuf,
}

impl WebcNode {
    pub fn new(inner: Arc<crate::fs::WebcFsInner>, path: VfsPathBuf) -> Self {
        Self { inner, path }
    }

    fn metadata_for(&self, path: &VfsPath) -> VfsResult<WebcMetadata> {
        let webc_path = WebcFs::to_webc_path(&self.inner, path)?;
        self.inner
            .volume
            .metadata(webc_path)
            .ok_or_else(|| VfsError::new(VfsErrorKind::NotFound, "webc.metadata"))
    }

    fn vfs_metadata_for(&self, path: &VfsPath) -> VfsResult<VfsMetadata> {
        let meta = self.metadata_for(path)?;
        let (file_type, size, modified) = match meta {
            WebcMetadata::Dir { timestamps } => {
                (VfsFileType::Directory, 0, get_modified(timestamps))
            }
            WebcMetadata::File {
                length, timestamps, ..
            } => (
                VfsFileType::RegularFile,
                length as u64,
                get_modified(timestamps),
            ),
        };

        Ok(VfsMetadata {
            inode: vfs_inode_for(path),
            file_type,
            mode: default_mode(file_type),
            nlink: 1,
            uid: 0,
            gid: 0,
            size,
            atime: modified,
            mtime: modified,
            ctime: modified,
            rdev_major: 0,
            rdev_minor: 0,
        })
    }

    fn lookup_path(&self, name: &VfsName) -> VfsPathBuf {
        let mut next = self.path.clone();
        next.push_name(*name);
        next
    }
}

impl FsNode for WebcNode {
    fn inode(&self) -> BackendInodeId {
        vfs_inode_for(self.path.as_path()).backend
    }

    fn file_type(&self) -> VfsFileType {
        match self.metadata_for(self.path.as_path()) {
            Ok(meta) => match meta {
                WebcMetadata::Dir { .. } => VfsFileType::Directory,
                WebcMetadata::File { .. } => VfsFileType::RegularFile,
            },
            Err(_) => VfsFileType::Unknown,
        }
    }

    fn metadata(&self) -> VfsResult<VfsMetadata> {
        self.vfs_metadata_for(self.path.as_path())
    }

    fn set_metadata(&self, _set: SetMetadata) -> VfsResult<()> {
        Err(VfsError::new(VfsErrorKind::ReadOnlyFs, "webc.set_metadata"))
    }

    fn lookup(&self, name: &VfsName) -> VfsResult<Arc<dyn FsNode>> {
        if self.file_type() != VfsFileType::Directory {
            return Err(VfsError::new(VfsErrorKind::NotDir, "webc.lookup.not_dir"));
        }
        let child_path = self.lookup_path(name);
        let _meta = self.metadata_for(child_path.as_path())?;
        Ok(Arc::new(WebcNode::new(self.inner.clone(), child_path)) as _)
    }

    fn create_file(&self, _name: &VfsName, _opts: CreateFile) -> VfsResult<Arc<dyn FsNode>> {
        Err(VfsError::new(VfsErrorKind::ReadOnlyFs, "webc.create_file"))
    }

    fn mkdir(&self, _name: &VfsName, _opts: MkdirOptions) -> VfsResult<Arc<dyn FsNode>> {
        Err(VfsError::new(VfsErrorKind::ReadOnlyFs, "webc.mkdir"))
    }

    fn unlink(&self, _name: &VfsName, _opts: UnlinkOptions) -> VfsResult<()> {
        Err(VfsError::new(VfsErrorKind::ReadOnlyFs, "webc.unlink"))
    }

    fn rmdir(&self, _name: &VfsName) -> VfsResult<()> {
        Err(VfsError::new(VfsErrorKind::ReadOnlyFs, "webc.rmdir"))
    }

    fn read_dir(&self, cursor: Option<DirCursor>, max: usize) -> VfsResult<ReadDirBatch> {
        if self.file_type() != VfsFileType::Directory {
            return Err(VfsError::new(VfsErrorKind::NotDir, "webc.readdir.not_dir"));
        }

        let webc_path = WebcFs::to_webc_path(&self.inner, self.path.as_path())?;
        let entries = self
            .inner
            .volume
            .read_dir(&webc_path)
            .ok_or_else(|| VfsError::new(VfsErrorKind::NotFound, "webc.readdir"))?;

        let start = cursor.map(|c| c.0 as usize).unwrap_or(0);
        let mut out = smallvec::SmallVec::new();
        for (idx, (name, _, meta)) in entries.into_iter().enumerate().skip(start) {
            if out.len() >= max {
                break;
            }
            let file_type = match meta {
                WebcMetadata::Dir { .. } => VfsFileType::Directory,
                WebcMetadata::File { .. } => VfsFileType::RegularFile,
            };
            let name_buf = VfsNameBuf::new(name.as_bytes().to_vec())
                .map_err(|_| VfsError::new(VfsErrorKind::InvalidInput, "webc.readdir.name"))?;
            out.push(vfs_core::dir::VfsDirEntry {
                name: name_buf,
                inode: None,
                file_type: Some(file_type),
            });
        }

        let next = if start + out.len() < entries.len() {
            Some(VfsDirCookie((start + out.len()) as u64))
        } else {
            None
        };

        Ok(ReadDirBatch { entries: out, next })
    }

    fn rename(
        &self,
        _old_name: &VfsName,
        _new_parent: &dyn FsNode,
        _new_name: &VfsName,
        _opts: RenameOptions,
    ) -> VfsResult<()> {
        Err(VfsError::new(VfsErrorKind::ReadOnlyFs, "webc.rename"))
    }

    fn open(&self, opts: OpenOptions) -> VfsResult<Arc<dyn FsHandle>> {
        if self.file_type() != VfsFileType::RegularFile {
            return Err(VfsError::new(VfsErrorKind::IsDir, "webc.open.not_file"));
        }
        if opts.flags.contains(vfs_core::flags::OpenFlags::WRITE)
            || opts.flags.contains(vfs_core::flags::OpenFlags::CREATE)
            || opts.flags.contains(vfs_core::flags::OpenFlags::TRUNC)
        {
            return Err(VfsError::new(VfsErrorKind::ReadOnlyFs, "webc.open.write"));
        }
        let webc_path = WebcFs::to_webc_path(&self.inner, self.path.as_path())?;
        let (bytes, _meta) = self
            .inner
            .volume
            .read_file(&webc_path)
            .ok_or_else(|| VfsError::new(VfsErrorKind::NotFound, "webc.open"))?;
        Ok(Arc::new(WebcFileHandle::new(bytes)))
    }

    fn link(&self, _existing: &dyn FsNode, _new_name: &VfsName) -> VfsResult<()> {
        Err(VfsError::new(VfsErrorKind::ReadOnlyFs, "webc.link"))
    }

    fn symlink(&self, _new_name: &VfsName, _target: &VfsPath) -> VfsResult<()> {
        Err(VfsError::new(VfsErrorKind::ReadOnlyFs, "webc.symlink"))
    }

    fn readlink(&self) -> VfsResult<VfsPathBuf> {
        Err(VfsError::new(VfsErrorKind::NotSupported, "webc.readlink"))
    }
}

fn vfs_inode_for(path: &VfsPath) -> vfs_core::VfsInodeId {
    let hash = xxhash_rust::xxh64::xxh64(path.as_bytes(), 0);
    let raw = if hash == 0 { 1 } else { hash };
    vfs_core::VfsInodeId {
        mount: vfs_core::MountId::from_index(0),
        backend: BackendInodeId::new(raw).expect("non-zero inode"),
    }
}

fn default_mode(file_type: VfsFileType) -> VfsFileMode {
    match file_type {
        VfsFileType::Directory => VfsFileMode(0o555),
        VfsFileType::RegularFile => VfsFileMode(0o444),
        _ => VfsFileMode(0o444),
    }
}

fn get_modified(timestamps: Option<webc::Timestamps>) -> VfsTimespec {
    let modified = timestamps.map(|t| t.modified()).unwrap_or_default();
    let nanos = modified % 1_000_000_000;
    let secs = (modified / 1_000_000_000) as i64;
    VfsTimespec {
        secs: secs.max(1),
        nanos: nanos as u32,
    }
}
