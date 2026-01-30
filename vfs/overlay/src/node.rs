use std::collections::HashSet;
use std::sync::{Arc, RwLock, Weak};

use smallvec::SmallVec;

use vfs_core::dir::VfsDirEntry;
use vfs_core::flags::{OpenFlags, OpenOptions};
use vfs_core::node::{
    CreateFile, DirCursor, FsHandle, FsNode, MkdirOptions, ReadDirBatch, RenameOptions,
    SetMetadata, UnlinkOptions, VfsDirCookie,
};
use vfs_core::path_types::{VfsName, VfsNameBuf, VfsPath, VfsPathBuf};
use vfs_core::{
    BackendInodeId, VfsError, VfsErrorKind, VfsFileType, VfsInodeId, VfsMetadata, VfsResult,
};

use crate::copy_up::{copy_up_dir, copy_up_file, copy_up_symlink};
use crate::fs::OverlayFs;
use crate::handle::OverlayHandle;
use crate::inodes::{BackingKey, OverlayInodeTable};
use crate::whiteout::{
    create_whiteout, deny_reserved_name, is_opaque_marker, is_reserved_name,
    is_reserved_name_bytes, is_whiteout_marker, opaque_name, whiteout_name_for,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverlayNodeKind {
    File,
    Dir,
    Symlink,
}

impl OverlayNodeKind {
    fn from_file_type(ty: VfsFileType) -> Self {
        match ty {
            VfsFileType::Directory => Self::Dir,
            VfsFileType::Symlink => Self::Symlink,
            _ => Self::File,
        }
    }

    fn as_file_type(self) -> VfsFileType {
        match self {
            Self::Dir => VfsFileType::Directory,
            Self::Symlink => VfsFileType::Symlink,
            Self::File => VfsFileType::RegularFile,
        }
    }
}

#[derive(Clone)]
pub struct LowerNode {
    pub layer: u16,
    pub node: Arc<dyn FsNode>,
}

impl LowerNode {
    pub fn to_key(&self) -> BackingKey {
        BackingKey::Lower {
            layer: self.layer,
            inode: self.node.inode(),
        }
    }
}

pub struct OverlayNode {
    self_ref: Weak<OverlayNode>,
    fs: Arc<OverlayFs>,
    kind: OverlayNodeKind,
    overlay_inode: BackendInodeId,
    upper: RwLock<Option<Arc<dyn FsNode>>>,
    lowers: SmallVec<[LowerNode; 2]>,
    parent: Option<Weak<OverlayNode>>,
    name: Option<VfsNameBuf>,
}

impl OverlayNode {
    pub(crate) fn new(
        fs: Arc<OverlayFs>,
        kind: OverlayNodeKind,
        overlay_inode: BackendInodeId,
        upper: Option<Arc<dyn FsNode>>,
        lowers: SmallVec<[LowerNode; 2]>,
        parent: Option<Weak<OverlayNode>>,
        name: Option<VfsNameBuf>,
    ) -> Arc<Self> {
        Arc::new_cyclic(|weak| Self {
            self_ref: weak.clone(),
            fs,
            kind,
            overlay_inode,
            upper: RwLock::new(upper),
            lowers,
            parent,
            name,
        })
    }

    fn upper(&self) -> Option<Arc<dyn FsNode>> {
        self.upper.read().expect("overlay upper lock").clone()
    }

    fn set_upper(&self, upper: Arc<dyn FsNode>, inodes: &OverlayInodeTable) {
        *self.upper.write().expect("overlay upper lock") = Some(upper.clone());
        inodes.promote(
            self.overlay_inode,
            BackingKey::Upper {
                inode: upper.inode(),
            },
        );
    }

    fn has_upper(&self) -> bool {
        self.upper.read().expect("overlay upper lock").is_some()
    }

    fn primary_lower(&self) -> Option<&LowerNode> {
        self.lowers.first()
    }

    fn is_dir(&self) -> bool {
        self.kind == OverlayNodeKind::Dir
    }

    fn parent(&self) -> Option<Arc<OverlayNode>> {
        self.parent.as_ref().and_then(|weak| weak.upgrade())
    }

    fn weak_self(&self) -> Weak<OverlayNode> {
        self.self_ref.clone()
    }

    fn require_name(&self) -> VfsResult<VfsNameBuf> {
        self.name
            .clone()
            .ok_or(VfsError::new(VfsErrorKind::InvalidInput, "overlay.name"))
    }

    fn active_node(&self) -> VfsResult<Arc<dyn FsNode>> {
        if let Some(upper) = self.upper() {
            return Ok(upper);
        }
        self.primary_lower()
            .map(|lower| lower.node.clone())
            .ok_or(VfsError::new(VfsErrorKind::NotFound, "overlay.active_node"))
    }

    fn is_opaque(&self) -> VfsResult<bool> {
        let upper = match self.upper() {
            Some(upper) => upper,
            None => return Ok(false),
        };
        let name = opaque_name()?;
        match upper.lookup(&VfsName::new(name.as_bytes())?) {
            Ok(_) => Ok(true),
            Err(err) if err.kind() == VfsErrorKind::NotFound => Ok(false),
            Err(err) => Err(err),
        }
    }

    fn has_whiteout(&self, name: &VfsName) -> VfsResult<bool> {
        let upper = match self.upper() {
            Some(upper) => upper,
            None => return Ok(false),
        };
        let wh = whiteout_name_for(name)?;
        match upper.lookup(&VfsName::new(wh.as_bytes())?) {
            Ok(_) => Ok(true),
            Err(err) if err.kind() == VfsErrorKind::NotFound => Ok(false),
            Err(err) => Err(err),
        }
    }

    fn ensure_upper_dir(&self) -> VfsResult<Arc<dyn FsNode>> {
        if let Some(upper) = self.upper() {
            return Ok(upper);
        }
        if !self.is_dir() {
            return Err(VfsError::new(
                VfsErrorKind::NotDir,
                "overlay.ensure_upper_dir",
            ));
        }
        let parent = self.parent().ok_or(VfsError::new(
            VfsErrorKind::InvalidInput,
            "overlay.ensure_upper_dir.parent",
        ))?;
        let parent_upper = parent.ensure_upper_dir()?;
        let name = self.require_name()?;
        let vfs_name = VfsName::new(name.as_bytes())?;

        let upper = match parent_upper.lookup(&vfs_name) {
            Ok(node) => node,
            Err(err) if err.kind() == VfsErrorKind::NotFound => {
                if let Some(lower) = self.primary_lower() {
                    copy_up_dir(&*parent_upper, &vfs_name, &*lower.node)?
                } else {
                    parent_upper.mkdir(&vfs_name, MkdirOptions { mode: None })?
                }
            }
            Err(err) => return Err(err),
        };
        self.set_upper(upper.clone(), &self.fs.inodes);
        Ok(upper)
    }

    fn ensure_upper_file(&self) -> VfsResult<Arc<dyn FsNode>> {
        if let Some(upper) = self.upper() {
            return Ok(upper);
        }
        let parent = self.parent().ok_or(VfsError::new(
            VfsErrorKind::InvalidInput,
            "overlay.ensure_upper_file.parent",
        ))?;
        let parent_upper = parent.ensure_upper_dir()?;
        let name = self.require_name()?;
        let vfs_name = VfsName::new(name.as_bytes())?;
        let lower = self.primary_lower().ok_or(VfsError::new(
            VfsErrorKind::NotFound,
            "overlay.ensure_upper_file.lower",
        ))?;
        let upper = copy_up_file(&*parent_upper, &vfs_name, &*lower.node, &self.fs.opts)?;
        self.set_upper(upper.clone(), &self.fs.inodes);
        Ok(upper)
    }

    fn ensure_upper_symlink(&self) -> VfsResult<Arc<dyn FsNode>> {
        if let Some(upper) = self.upper() {
            return Ok(upper);
        }
        let parent = self.parent().ok_or(VfsError::new(
            VfsErrorKind::InvalidInput,
            "overlay.ensure_upper_symlink.parent",
        ))?;
        let parent_upper = parent.ensure_upper_dir()?;
        let name = self.require_name()?;
        let vfs_name = VfsName::new(name.as_bytes())?;
        let lower = self.primary_lower().ok_or(VfsError::new(
            VfsErrorKind::NotFound,
            "overlay.ensure_upper_symlink.lower",
        ))?;
        let upper = copy_up_symlink(&*parent_upper, &vfs_name, &*lower.node)?;
        self.set_upper(upper.clone(), &self.fs.inodes);
        Ok(upper)
    }

    fn read_dir_all(node: &dyn FsNode) -> VfsResult<Vec<VfsDirEntry>> {
        let mut cursor = None;
        let mut entries = Vec::new();
        loop {
            let batch = node.read_dir(cursor, 128)?;
            entries.extend(batch.entries.into_iter());
            cursor = batch.next;
            if cursor.is_none() {
                break;
            }
        }
        Ok(entries)
    }

    fn dir_empty(node: &dyn FsNode) -> VfsResult<bool> {
        let batch = node.read_dir(None, 1)?;
        Ok(batch.entries.is_empty())
    }

    fn lookup_lower_first(&self, name: &VfsName) -> VfsResult<Option<LowerNode>> {
        for lower in &self.lowers {
            match lower.node.lookup(name) {
                Ok(node) => {
                    return Ok(Some(LowerNode {
                        layer: lower.layer,
                        node,
                    }));
                }
                Err(err) if err.kind() == VfsErrorKind::NotFound => continue,
                Err(err) => return Err(err),
            }
        }
        Ok(None)
    }

    fn lower_dirs_for_name(&self, name: &VfsName) -> VfsResult<SmallVec<[LowerNode; 2]>> {
        let mut dirs = SmallVec::new();
        for lower in &self.lowers {
            match lower.node.lookup(name) {
                Ok(node) => {
                    if node.file_type() == VfsFileType::Directory {
                        dirs.push(LowerNode {
                            layer: lower.layer,
                            node,
                        });
                    }
                }
                Err(err) if err.kind() == VfsErrorKind::NotFound => continue,
                Err(err) => return Err(err),
            }
        }
        Ok(dirs)
    }

    fn upper_entry_exists(&self, name: &VfsName) -> VfsResult<bool> {
        let upper = match self.upper() {
            Some(upper) => upper,
            None => return Ok(false),
        };
        match upper.lookup(name) {
            Ok(_) => Ok(true),
            Err(err) if err.kind() == VfsErrorKind::NotFound => Ok(false),
            Err(err) => Err(err),
        }
    }

    fn lower_entry_exists(&self, name: &VfsName) -> VfsResult<bool> {
        Ok(self.lookup_lower_first(name)?.is_some())
    }

    fn deny_reserved(&self, name: &VfsName) -> VfsResult<()> {
        if self.fs.opts.deny_reserved_names {
            deny_reserved_name(name)?;
        }
        Ok(())
    }
}

impl FsNode for OverlayNode {
    fn inode(&self) -> BackendInodeId {
        self.overlay_inode
    }

    fn file_type(&self) -> VfsFileType {
        self.kind.as_file_type()
    }

    fn metadata(&self) -> VfsResult<VfsMetadata> {
        let meta = self.active_node()?.metadata()?;
        Ok(VfsMetadata {
            inode: VfsInodeId {
                mount: meta.inode.mount,
                backend: self.overlay_inode,
            },
            file_type: meta.file_type,
            mode: meta.mode,
            uid: meta.uid,
            gid: meta.gid,
            nlink: meta.nlink,
            size: meta.size,
            atime: meta.atime,
            mtime: meta.mtime,
            ctime: meta.ctime,
            rdev_major: meta.rdev_major,
            rdev_minor: meta.rdev_minor,
        })
    }

    fn set_metadata(&self, set: SetMetadata) -> VfsResult<()> {
        match self.kind {
            OverlayNodeKind::Dir => {
                let upper = self.ensure_upper_dir()?;
                upper.set_metadata(set)
            }
            OverlayNodeKind::Symlink => {
                let upper = self.ensure_upper_symlink()?;
                upper.set_metadata(set)
            }
            OverlayNodeKind::File => {
                let upper = self.ensure_upper_file()?;
                upper.set_metadata(set)
            }
        }
    }

    fn lookup(&self, name: &VfsName) -> VfsResult<Arc<dyn FsNode>> {
        if is_reserved_name(name) {
            return Err(VfsError::new(
                VfsErrorKind::NotFound,
                "overlay.lookup.reserved",
            ));
        }
        if let Some(upper) = self.upper() {
            if let Ok(child) = upper.lookup(name) {
                let child_kind = OverlayNodeKind::from_file_type(child.file_type());
                let lowers = if child.file_type() == VfsFileType::Directory && !self.is_opaque()? {
                    self.lower_dirs_for_name(name)?
                } else {
                    SmallVec::new()
                };
                let child_name = VfsNameBuf::new(name.as_bytes().to_vec())?;
                let node = self.fs.make_node(
                    Some(child_name),
                    Some(self.weak_self()),
                    child_kind,
                    Some(child),
                    lowers,
                );
                return Ok(node);
            }
            if self.has_whiteout(name)? {
                return Err(VfsError::new(
                    VfsErrorKind::NotFound,
                    "overlay.lookup.whiteout",
                ));
            }
        }

        if self.is_opaque()? {
            return Err(VfsError::new(
                VfsErrorKind::NotFound,
                "overlay.lookup.opaque",
            ));
        }

        if let Some(lower) = self.lookup_lower_first(name)? {
            let kind = OverlayNodeKind::from_file_type(lower.node.file_type());
            let mut lowers = SmallVec::new();
            lowers.push(lower);
            let child_name = VfsNameBuf::new(name.as_bytes().to_vec())?;
            let node =
                self.fs
                    .make_node(Some(child_name), Some(self.weak_self()), kind, None, lowers);
            return Ok(node);
        }
        Err(VfsError::new(VfsErrorKind::NotFound, "overlay.lookup.miss"))
    }

    fn create_file(&self, name: &VfsName, opts: CreateFile) -> VfsResult<Arc<dyn FsNode>> {
        self.deny_reserved(name)?;
        let upper = self.ensure_upper_dir()?;
        let node = upper.create_file(name, opts)?;
        let child_name = VfsNameBuf::new(name.as_bytes().to_vec())?;
        let overlay = self.fs.make_node(
            Some(child_name),
            Some(self.weak_self()),
            OverlayNodeKind::File,
            Some(node),
            SmallVec::new(),
        );
        Ok(overlay)
    }

    fn mkdir(&self, name: &VfsName, opts: MkdirOptions) -> VfsResult<Arc<dyn FsNode>> {
        self.deny_reserved(name)?;
        let upper = self.ensure_upper_dir()?;
        let node = upper.mkdir(name, opts)?;
        let child_name = VfsNameBuf::new(name.as_bytes().to_vec())?;
        let overlay = self.fs.make_node(
            Some(child_name),
            Some(self.weak_self()),
            OverlayNodeKind::Dir,
            Some(node),
            SmallVec::new(),
        );
        Ok(overlay)
    }

    fn unlink(&self, name: &VfsName, opts: UnlinkOptions) -> VfsResult<()> {
        if is_reserved_name(name) {
            return Err(VfsError::new(
                VfsErrorKind::NotFound,
                "overlay.unlink.reserved",
            ));
        }
        let upper = self.upper();
        let lower_exists = self.lower_entry_exists(name)?;
        if let Some(upper) = upper {
            if let Ok(existing) = upper.lookup(name) {
                if opts.must_be_dir && existing.file_type() != VfsFileType::Directory {
                    return Err(VfsError::new(VfsErrorKind::NotDir, "overlay.unlink"));
                }
                if !opts.must_be_dir && existing.file_type() == VfsFileType::Directory {
                    return Err(VfsError::new(VfsErrorKind::IsDir, "overlay.unlink"));
                }
                if existing.file_type() == VfsFileType::Directory {
                    upper.rmdir(name)?;
                } else {
                    upper.unlink(name, opts)?;
                }
                if lower_exists {
                    create_whiteout(&*upper, name)?;
                }
                return Ok(());
            }
        }

        if lower_exists {
            let upper = self.ensure_upper_dir()?;
            create_whiteout(&*upper, name)?;
            return Ok(());
        }

        Err(VfsError::new(VfsErrorKind::NotFound, "overlay.unlink.miss"))
    }

    fn rmdir(&self, name: &VfsName) -> VfsResult<()> {
        if is_reserved_name(name) {
            return Err(VfsError::new(
                VfsErrorKind::NotFound,
                "overlay.rmdir.reserved",
            ));
        }
        let upper = self.upper();
        let lower_exists = self.lower_entry_exists(name)?;

        if let Some(upper) = upper {
            if let Ok(existing) = upper.lookup(name) {
                if existing.file_type() != VfsFileType::Directory {
                    return Err(VfsError::new(VfsErrorKind::NotDir, "overlay.rmdir"));
                }
                let overlay_dir = self.lookup(name)?;
                if !Self::dir_empty(&*overlay_dir)? {
                    return Err(VfsError::new(VfsErrorKind::DirNotEmpty, "overlay.rmdir"));
                }
                upper.rmdir(name)?;
                if lower_exists {
                    create_whiteout(&*upper, name)?;
                }
                return Ok(());
            }
        }

        if lower_exists {
            let overlay_dir = self.lookup(name)?;
            if !Self::dir_empty(&*overlay_dir)? {
                return Err(VfsError::new(VfsErrorKind::DirNotEmpty, "overlay.rmdir"));
            }
            let upper = self.ensure_upper_dir()?;
            create_whiteout(&*upper, name)?;
            return Ok(());
        }

        Err(VfsError::new(VfsErrorKind::NotFound, "overlay.rmdir.miss"))
    }

    fn read_dir(&self, cursor: Option<DirCursor>, max: usize) -> VfsResult<ReadDirBatch> {
        if !self.is_dir() {
            return Err(VfsError::new(VfsErrorKind::NotDir, "overlay.read_dir"));
        }
        let mut entries: Vec<VfsDirEntry> = Vec::new();
        let mut seen = HashSet::new();
        let mut whiteouts = HashSet::new();
        let mut opaque = false;

        if let Some(upper) = self.upper() {
            for entry in Self::read_dir_all(&*upper)? {
                let name_bytes = entry.name.as_bytes();
                if is_opaque_marker(name_bytes) {
                    opaque = true;
                    continue;
                }
                if let Some(stripped) = is_whiteout_marker(name_bytes) {
                    whiteouts.insert(stripped.to_vec());
                    continue;
                }
                if is_reserved_name_bytes(name_bytes) {
                    continue;
                }
                seen.insert(name_bytes.to_vec());
                let vfs_name = VfsName::new(name_bytes)?;
                let node = self.lookup(&vfs_name)?;
                entries.push(VfsDirEntry {
                    name: VfsNameBuf::new(name_bytes.to_vec())?,
                    inode: Some(node.inode()),
                    file_type: Some(node.file_type()),
                });
            }
        }

        if !opaque {
            for lower in &self.lowers {
                for entry in Self::read_dir_all(&*lower.node)? {
                    let name_bytes = entry.name.as_bytes();
                    if is_reserved_name_bytes(name_bytes) {
                        continue;
                    }
                    if seen.contains(name_bytes) {
                        continue;
                    }
                    if whiteouts.contains(name_bytes) {
                        continue;
                    }
                    seen.insert(name_bytes.to_vec());
                    let vfs_name = VfsName::new(name_bytes)?;
                    let node = self.lookup(&vfs_name)?;
                    entries.push(VfsDirEntry {
                        name: VfsNameBuf::new(name_bytes.to_vec())?,
                        inode: Some(node.inode()),
                        file_type: Some(node.file_type()),
                    });
                }
            }
        }

        let start = cursor.map(|c| c.0 as usize).unwrap_or(0);
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
        self.deny_reserved(new_name)?;
        if opts.exchange {
            return Err(VfsError::new(
                VfsErrorKind::NotSupported,
                "overlay.rename.exchange",
            ));
        }
        let new_parent = new_parent
            .as_any()
            .downcast_ref::<OverlayNode>()
            .ok_or(VfsError::new(VfsErrorKind::CrossDevice, "overlay.rename"))?;

        if opts.noreplace {
            if new_parent.lookup(new_name).is_ok() {
                return Err(VfsError::new(
                    VfsErrorKind::AlreadyExists,
                    "overlay.rename.noreplace",
                ));
            }
        }

        let src = self.lookup(old_name)?;
        let src = src
            .as_any()
            .downcast_ref::<OverlayNode>()
            .ok_or(VfsError::new(VfsErrorKind::Internal, "overlay.rename.src"))?;

        let src_lower_only = !src.has_upper();
        if src_lower_only && src.kind == OverlayNodeKind::Dir {
            return Err(VfsError::new(
                VfsErrorKind::NotSupported,
                "overlay.rename.lower_dir",
            ));
        }

        if src_lower_only {
            match src.kind {
                OverlayNodeKind::File => {
                    src.ensure_upper_file()?;
                }
                OverlayNodeKind::Symlink => {
                    src.ensure_upper_symlink()?;
                }
                OverlayNodeKind::Dir => {}
            }
        }

        let upper_src_parent = self.ensure_upper_dir()?;
        let upper_dst_parent = new_parent.ensure_upper_dir()?;

        let dest_lower_exists = new_parent.lower_entry_exists(new_name)?;
        let dest_upper_exists = new_parent.upper_entry_exists(new_name)?;

        upper_src_parent.rename(old_name, &*upper_dst_parent, new_name, opts)?;

        if src_lower_only {
            create_whiteout(&*upper_src_parent, old_name)?;
        }

        if dest_lower_exists && !dest_upper_exists {
            create_whiteout(&*upper_dst_parent, new_name)?;
        }

        Ok(())
    }

    fn link(&self, existing: &dyn FsNode, new_name: &VfsName) -> VfsResult<()> {
        self.deny_reserved(new_name)?;
        let existing = existing
            .as_any()
            .downcast_ref::<OverlayNode>()
            .ok_or(VfsError::new(VfsErrorKind::CrossDevice, "overlay.link"))?;
        if existing.kind == OverlayNodeKind::Dir {
            return Err(VfsError::new(
                VfsErrorKind::NotSupported,
                "overlay.link.dir",
            ));
        }
        let upper_existing = match existing.kind {
            OverlayNodeKind::File => existing.ensure_upper_file()?,
            OverlayNodeKind::Symlink => existing.ensure_upper_symlink()?,
            OverlayNodeKind::Dir => unreachable!(),
        };
        let upper_parent = self.ensure_upper_dir()?;
        upper_parent.link(&*upper_existing, new_name)
    }

    fn symlink(&self, new_name: &VfsName, target: &VfsPath) -> VfsResult<()> {
        self.deny_reserved(new_name)?;
        let upper = self.ensure_upper_dir()?;
        upper.symlink(new_name, target)
    }

    fn readlink(&self) -> VfsResult<VfsPathBuf> {
        self.active_node()?.readlink()
    }

    fn open(&self, opts: OpenOptions) -> VfsResult<Arc<dyn FsHandle>> {
        let write_intent = opts.flags.intersects(
            OpenFlags::WRITE | OpenFlags::TRUNC | OpenFlags::CREATE | OpenFlags::APPEND,
        );
        let node = match self.kind {
            OverlayNodeKind::Dir => self.active_node()?,
            OverlayNodeKind::Symlink => {
                if write_intent {
                    self.ensure_upper_symlink()?
                } else {
                    self.active_node()?
                }
            }
            OverlayNodeKind::File => {
                if write_intent {
                    self.ensure_upper_file()?
                } else {
                    self.active_node()?
                }
            }
        };
        let handle = node.open(OpenOptions {
            flags: opts.flags,
            mode: opts.mode,
            resolve: opts.resolve,
        })?;
        Ok(Arc::new(OverlayHandle::new(handle)))
    }
}
