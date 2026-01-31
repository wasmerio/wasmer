use std::sync::Arc;

use vfs_core::dir::VfsDirEntry;
use vfs_core::flags::{OpenFlags, OpenOptions};
use vfs_core::node::{
    CreateFile, DirCursor, MkdirOptions, ReadDirBatch, RenameOptions, SetMetadata, UnlinkOptions,
    VfsDirCookie,
};
use vfs_core::path_types::{VfsName, VfsNameBuf, VfsPath, VfsPathBuf};
use vfs_core::{
    BackendInodeId, MountId, VfsError, VfsErrorKind, VfsFileMode, VfsFileType, VfsMetadata,
    VfsResult,
};

use crate::fs::HostFsInner;
use crate::handle::HostHandle;
use crate::platform::{self, DirEntryInfo};

#[derive(Debug)]
pub struct HostNode {
    pub(crate) fs: Arc<HostFsInner>,
    pub(crate) kind: HostNodeKind,
}

#[derive(Debug)]
pub(crate) enum HostNodeKind {
    Dir(Arc<HostDir>),
    File { inode: BackendInodeId, locator: Locator },
    Symlink { inode: BackendInodeId, locator: Locator },
}

#[derive(Debug)]
pub(crate) struct HostDir {
    pub(crate) dir: platform::DirHandle,
    pub(crate) inode: BackendInodeId,
    pub(crate) locator: Locator,
}

#[derive(Clone, Debug)]
pub(crate) struct Locator {
    pub(crate) parent: Option<Arc<HostDir>>,
    pub(crate) name: Option<VfsNameBuf>,
}

impl HostNode {
    fn is_read_only(&self) -> bool {
        self.fs
            .mount_flags
            .contains(vfs_core::provider::MountFlags::READ_ONLY)
    }

    fn ensure_dir(&self) -> VfsResult<&Arc<HostDir>> {
        match &self.kind {
            HostNodeKind::Dir(dir) => Ok(dir),
            _ => Err(VfsError::new(VfsErrorKind::NotDir, "host.node.ensure_dir")),
        }
    }

    fn file_locator(&self) -> VfsResult<&Locator> {
        match &self.kind {
            HostNodeKind::Dir(dir) => Ok(&dir.locator),
            HostNodeKind::File { locator, .. } => Ok(locator),
            HostNodeKind::Symlink { locator, .. } => Ok(locator),
        }
    }

    fn locate_parent(&self, context: &'static str) -> VfsResult<(&Arc<HostDir>, &VfsNameBuf)> {
        let locator = self.file_locator()?;
        let parent = locator
            .parent
            .as_ref()
            .ok_or_else(|| VfsError::new(VfsErrorKind::InvalidInput, context))?;
        let name = locator
            .name
            .as_ref()
            .ok_or_else(|| VfsError::new(VfsErrorKind::InvalidInput, context))?;
        Ok((parent, name))
    }

    fn node_from_stat(
        &self,
        parent: Arc<HostDir>,
        name: VfsNameBuf,
        stat: platform::Stat,
    ) -> Arc<HostNode> {
        let locator = Locator {
            parent: Some(parent),
            name: Some(name),
        };
        let kind = match stat.file_type {
            VfsFileType::Directory => {
                let dir = HostDir {
                    dir: stat.dir_handle.expect("dir handle"),
                    inode: stat.inode,
                    locator,
                };
                HostNodeKind::Dir(Arc::new(dir))
            }
            VfsFileType::Symlink => HostNodeKind::Symlink {
                inode: stat.inode,
                locator,
            },
            _ => HostNodeKind::File {
                inode: stat.inode,
                locator,
            },
        };
        Arc::new(HostNode {
            fs: self.fs.clone(),
            kind,
        })
    }
}

impl vfs_core::traits_sync::FsNodeSync for HostNode {
    fn inode(&self) -> BackendInodeId {
        match &self.kind {
            HostNodeKind::Dir(dir) => dir.inode,
            HostNodeKind::File { inode, .. } => *inode,
            HostNodeKind::Symlink { inode, .. } => *inode,
        }
    }

    fn file_type(&self) -> VfsFileType {
        match &self.kind {
            HostNodeKind::Dir(_) => VfsFileType::Directory,
            HostNodeKind::File { .. } => VfsFileType::RegularFile,
            HostNodeKind::Symlink { .. } => VfsFileType::Symlink,
        }
    }

    fn metadata(&self) -> VfsResult<VfsMetadata> {
        let stat = match &self.kind {
            HostNodeKind::Dir(dir) => {
                crate::io_result("host.metadata.dir", platform::stat_dir(&dir.dir))?
            }
            HostNodeKind::File { .. } | HostNodeKind::Symlink { .. } => {
                let (parent, name) = self.locate_parent("host.metadata.locator")?;
                crate::io_result(
                    "host.metadata.entry",
                    platform::stat_at(&parent.dir, name.as_bytes(), true),
                )?
            }
        };
        Ok(metadata_from_stat(&stat))
    }

    fn set_metadata(&self, set: SetMetadata) -> VfsResult<()> {
        if self.is_read_only() {
            return Err(crate::readonly_error("host.set_metadata.read_only"));
        }

        if let Some(size) = set.size {
            match self.file_type() {
                VfsFileType::RegularFile => {
                    let handle = self.open(OpenOptions {
                        flags: OpenFlags::WRITE,
                        mode: None,
                        resolve: vfs_core::flags::ResolveFlags::empty(),
                    })?;
                    handle.set_len(size)?;
                }
                VfsFileType::Directory => {
                    return Err(VfsError::new(VfsErrorKind::InvalidInput, "host.set_metadata.size"));
                }
                VfsFileType::Symlink => {
                    return Err(VfsError::new(
                        VfsErrorKind::NotSupported,
                        "host.set_metadata.size_symlink",
                    ));
                }
                _ => {
                    return Err(VfsError::new(
                        VfsErrorKind::NotSupported,
                        "host.set_metadata.size_other",
                    ));
                }
            }
        }

        let (parent, name) = match self.file_type() {
            VfsFileType::Directory => {
                let locator = self.file_locator()?;
                let Some(parent) = locator.parent.as_ref() else {
                    return Ok(());
                };
                let Some(name) = locator.name.as_ref() else {
                    return Ok(());
                };
                (parent, name)
            }
            _ => self.locate_parent("host.set_metadata.locator")?,
        };

        if let Some(mode) = set.mode {
            crate::io_result(
                "host.set_metadata.chmod",
                platform::chmod_at(&parent.dir, name.as_bytes(), mode.bits()),
            )?;
        }
        if set.uid.is_some() || set.gid.is_some() {
            crate::io_result(
                "host.set_metadata.chown",
                platform::chown_at(&parent.dir, name.as_bytes(), set.uid, set.gid),
            )?;
        }
        if set.atime.is_some() || set.mtime.is_some() {
            crate::io_result(
                "host.set_metadata.utimens",
                platform::utimens_at(&parent.dir, name.as_bytes(), set.atime, set.mtime),
            )?;
        }
        if set.ctime.is_some() {
            return Err(VfsError::new(
                VfsErrorKind::NotSupported,
                "host.set_metadata.ctime",
            ));
        }
        Ok(())
    }

    fn lookup(&self, name: &VfsName) -> VfsResult<Arc<dyn vfs_core::traits_sync::FsNodeSync>> {
        let dir = self.ensure_dir()?;
        let stat = crate::io_result(
            "host.lookup.stat",
            platform::stat_at(&dir.dir, name.as_bytes(), true),
        )?;
        let mut stat = stat;
        if stat.file_type == VfsFileType::Directory {
            let handle =
                crate::io_result("host.lookup.open_dir", platform::open_dir_at(&dir.dir, name))?;
            stat.dir_handle = Some(handle);
        }
        let name_buf = VfsNameBuf::new(name.as_bytes().to_vec())
            .map_err(|_| VfsError::new(VfsErrorKind::InvalidInput, "host.lookup.name"))?;
        Ok(self.node_from_stat(dir.clone(), name_buf, stat))
    }

    fn create_file(&self, name: &VfsName, opts: CreateFile) -> VfsResult<Arc<dyn vfs_core::traits_sync::FsNodeSync>> {
        if self.is_read_only() {
            return Err(crate::readonly_error("host.create_file.read_only"));
        }
        let dir = self.ensure_dir()?;
        match platform::stat_at(&dir.dir, name.as_bytes(), true) {
            Ok(stat) => {
                if stat.file_type == VfsFileType::Directory {
                    return Err(VfsError::new(VfsErrorKind::IsDir, "host.create_file.isdir"));
                }
                if opts.exclusive {
                    return Err(VfsError::new(
                        VfsErrorKind::AlreadyExists,
                        "host.create_file.exists",
                    ));
                }
                if opts.truncate {
                    let flags = OpenFlags::WRITE | OpenFlags::TRUNC | OpenFlags::NOFOLLOW;
                    let _ = crate::io_result(
                        "host.create_file.trunc",
                        platform::open_file_at(&dir.dir, name.as_bytes(), flags, opts.mode),
                    )?;
                }
                let name_buf = VfsNameBuf::new(name.as_bytes().to_vec())
                    .map_err(|_| VfsError::new(VfsErrorKind::InvalidInput, "host.create_file.name"))?;
                return Ok(self.node_from_stat(dir.clone(), name_buf, stat));
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(crate::map_io_error("host.create_file.stat", err));
            }
        }

        let mut flags = OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::NOFOLLOW;
        if opts.exclusive {
            flags |= OpenFlags::EXCL;
        }
        if opts.truncate {
            flags |= OpenFlags::TRUNC;
        }
        let _ = crate::io_result(
            "host.create_file.create",
            platform::open_file_at(&dir.dir, name.as_bytes(), flags, opts.mode),
        )?;
        let stat = crate::io_result(
            "host.create_file.stat",
            platform::stat_at(&dir.dir, name.as_bytes(), true),
        )?;
        let name_buf = VfsNameBuf::new(name.as_bytes().to_vec())
            .map_err(|_| VfsError::new(VfsErrorKind::InvalidInput, "host.create_file.name"))?;
        Ok(self.node_from_stat(dir.clone(), name_buf, stat))
    }

    fn mkdir(&self, name: &VfsName, opts: MkdirOptions) -> VfsResult<Arc<dyn vfs_core::traits_sync::FsNodeSync>> {
        if self.is_read_only() {
            return Err(crate::readonly_error("host.mkdir.read_only"));
        }
        let dir = self.ensure_dir()?;
        match platform::stat_at(&dir.dir, name.as_bytes(), true) {
            Ok(_) => {
                return Err(VfsError::new(VfsErrorKind::AlreadyExists, "host.mkdir.exists"));
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(crate::map_io_error("host.mkdir.stat", err));
            }
        }
        crate::io_result(
            "host.mkdir",
            platform::mkdir_at(&dir.dir, name.as_bytes(), opts.mode),
        )?;
        let mut stat = crate::io_result(
            "host.mkdir.stat",
            platform::stat_at(&dir.dir, name.as_bytes(), true),
        )?;
        let handle =
            crate::io_result("host.mkdir.open_dir", platform::open_dir_at(&dir.dir, name))?;
        stat.dir_handle = Some(handle);
        let name_buf = VfsNameBuf::new(name.as_bytes().to_vec())
            .map_err(|_| VfsError::new(VfsErrorKind::InvalidInput, "host.mkdir.name"))?;
        Ok(self.node_from_stat(dir.clone(), name_buf, stat))
    }

    fn unlink(&self, name: &VfsName, opts: UnlinkOptions) -> VfsResult<()> {
        if self.is_read_only() {
            return Err(crate::readonly_error("host.unlink.read_only"));
        }
        let dir = self.ensure_dir()?;
        if opts.must_be_dir {
            return self.rmdir(name);
        }
        crate::io_result(
            "host.unlink",
            platform::unlink_at(&dir.dir, name.as_bytes()),
        )?;
        Ok(())
    }

    fn rmdir(&self, name: &VfsName) -> VfsResult<()> {
        if self.is_read_only() {
            return Err(crate::readonly_error("host.rmdir.read_only"));
        }
        let dir = self.ensure_dir()?;
        crate::io_result(
            "host.rmdir",
            platform::rmdir_at(&dir.dir, name.as_bytes()),
        )?;
        Ok(())
    }

    fn read_dir(&self, cursor: Option<DirCursor>, max: usize) -> VfsResult<ReadDirBatch> {
        let dir = self.ensure_dir()?;
        let entries = crate::io_result("host.read_dir", platform::read_dir(&dir.dir))?;
        let mut entries: Vec<DirEntryInfo> = entries
            .into_iter()
            .filter(|entry| entry.name.as_slice() != b"." && entry.name.as_slice() != b"..")
            .collect();
        entries.sort_by(|a, b| a.name.cmp(&b.name));

        let start = cursor.map(|c| c.0 as usize).unwrap_or(0);
        if start >= entries.len() {
            return Ok(ReadDirBatch {
                entries: Default::default(),
                next: None,
            });
        }
        let end = entries.len().min(start + max);
        let mut batch = ReadDirBatch {
            entries: Default::default(),
            next: None,
        };
        for entry in &entries[start..end] {
            let name = VfsNameBuf::new(entry.name.clone())
                .map_err(|_| VfsError::new(VfsErrorKind::InvalidInput, "host.read_dir.name"))?;
            batch.entries.push(VfsDirEntry {
                name,
                inode: Some(entry.inode),
                file_type: Some(entry.file_type),
            });
        }
        if end < entries.len() {
            batch.next = Some(VfsDirCookie(end as u64));
        }
        Ok(batch)
    }

    fn rename(
        &self,
        old_name: &VfsName,
        new_parent: &dyn vfs_core::traits_sync::FsNodeSync,
        new_name: &VfsName,
        opts: RenameOptions,
    ) -> VfsResult<()> {
        if self.is_read_only() {
            return Err(crate::readonly_error("host.rename.read_only"));
        }
        if opts.exchange {
            return Err(VfsError::new(
                VfsErrorKind::NotSupported,
                "host.rename.exchange",
            ));
        }
        let dir = self.ensure_dir()?;
        let new_parent = new_parent
            .as_any()
            .downcast_ref::<HostNode>()
            .ok_or_else(|| VfsError::new(VfsErrorKind::CrossDevice, "host.rename.cross_device"))?;
        if !Arc::ptr_eq(&self.fs, &new_parent.fs) {
            return Err(VfsError::new(VfsErrorKind::CrossDevice, "host.rename.cross_device"));
        }
        let new_dir = new_parent.ensure_dir()?;

        if opts.noreplace {
            match platform::stat_at(&new_dir.dir, new_name.as_bytes(), true) {
                Ok(_) => {
                    return Err(VfsError::new(
                        VfsErrorKind::AlreadyExists,
                        "host.rename.noreplace",
                    ));
                }
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => {
                    return Err(crate::map_io_error("host.rename.stat", err));
                }
            }
        }

        crate::io_result(
            "host.rename",
            platform::rename_at(&dir.dir, old_name.as_bytes(), &new_dir.dir, new_name.as_bytes()),
        )?;
        Ok(())
    }

    fn link(&self, existing: &dyn vfs_core::traits_sync::FsNodeSync, new_name: &VfsName) -> VfsResult<()> {
        if !self.fs.caps.contains(vfs_core::VfsCapabilities::HARDLINKS) {
            return Err(VfsError::new(VfsErrorKind::NotSupported, "host.link.unsupported"));
        }
        if self.is_read_only() {
            return Err(crate::readonly_error("host.link.read_only"));
        }
        let dir = self.ensure_dir()?;
        let existing = existing
            .as_any()
            .downcast_ref::<HostNode>()
            .ok_or_else(|| VfsError::new(VfsErrorKind::CrossDevice, "host.link.cross_device"))?;
        if !Arc::ptr_eq(&self.fs, &existing.fs) {
            return Err(VfsError::new(VfsErrorKind::CrossDevice, "host.link.cross_device"));
        }
        let (parent, name) = existing.locate_parent("host.link.locator")?;
        crate::io_result(
            "host.link",
            platform::link_at(&parent.dir, name.as_bytes(), &dir.dir, new_name.as_bytes()),
        )?;
        Ok(())
    }

    fn symlink(&self, new_name: &VfsName, target: &VfsPath) -> VfsResult<()> {
        if !self.fs.caps.contains(vfs_core::VfsCapabilities::SYMLINKS) {
            return Err(VfsError::new(
                VfsErrorKind::NotSupported,
                "host.symlink.unsupported",
            ));
        }
        if self.is_read_only() {
            return Err(crate::readonly_error("host.symlink.read_only"));
        }
        let dir = self.ensure_dir()?;
        if target.as_bytes().iter().any(|b| *b == 0) {
            return Err(VfsError::new(VfsErrorKind::InvalidInput, "host.symlink.target"));
        }
        crate::io_result(
            "host.symlink",
            platform::symlink_at(&dir.dir, new_name.as_bytes(), target.as_bytes()),
        )?;
        Ok(())
    }

    fn readlink(&self) -> VfsResult<VfsPathBuf> {
        if !self.fs.caps.contains(vfs_core::VfsCapabilities::SYMLINKS) {
            return Err(VfsError::new(
                VfsErrorKind::NotSupported,
                "host.readlink.unsupported",
            ));
        }
        let (parent, name) = self.locate_parent("host.readlink.locator")?;
        let target =
            crate::io_result("host.readlink", platform::readlink_at(&parent.dir, name.as_bytes()))?;
        Ok(VfsPathBuf::from_bytes(target))
    }

    fn open(&self, opts: OpenOptions) -> VfsResult<Arc<dyn vfs_core::traits_sync::FsHandleSync>> {
        if self.file_type() != VfsFileType::RegularFile {
            return Err(VfsError::new(VfsErrorKind::IsDir, "host.open.not_file"));
        }
        if self.is_read_only()
            && (opts.flags.contains(OpenFlags::WRITE)
                || opts.flags.contains(OpenFlags::APPEND)
                || opts.flags.contains(OpenFlags::CREATE)
                || opts.flags.contains(OpenFlags::TRUNC)
                || opts.flags.contains(OpenFlags::EXCL))
        {
            return Err(crate::readonly_error("host.open.read_only"));
        }
        let (parent, name) = self.locate_parent("host.open.locator")?;
        let flags = opts.flags | OpenFlags::NOFOLLOW;
        let file = crate::io_result(
            "host.open.file",
            platform::open_file_at(&parent.dir, name.as_bytes(), flags, opts.mode),
        )?;
        Ok(Arc::new(HostHandle::new(file)))
    }
}

pub(crate) fn metadata_from_stat(stat: &platform::Stat) -> VfsMetadata {
    VfsMetadata {
        inode: vfs_core::VfsInodeId {
            mount: MountId::from_index(0),
            backend: stat.inode,
        },
        file_type: stat.file_type,
        mode: VfsFileMode(stat.mode),
        uid: stat.uid,
        gid: stat.gid,
        nlink: stat.nlink,
        size: stat.size,
        atime: stat.atime,
        mtime: stat.mtime,
        ctime: stat.ctime,
        rdev_major: stat.rdev_major,
        rdev_minor: stat.rdev_minor,
    }
}
