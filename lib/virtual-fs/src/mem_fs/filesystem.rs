//! This module contains the [`FileSystem`] type itself.

use super::*;
use crate::{DirEntry, FileType, FsError, Metadata, OpenOptions, ReadDir, Result};
use futures::future::BoxFuture;
use slab::Slab;
use std::ffi::OsString;
use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, RwLock};

/// The in-memory file system!
///
/// This `FileSystem` type can be cloned, it's a light copy of the
/// `FileSystemInner` (which is behind a `Arc` + `RwLock`).
#[derive(Default, Clone)]
pub struct FileSystem {
    pub(super) inner: Arc<RwLock<FileSystemInner>>,
}

impl FileSystem {
    pub fn set_memory_limiter(&self, limiter: crate::limiter::DynFsMemoryLimiter) {
        self.inner.write().unwrap().limiter = Some(limiter);
    }

    pub fn new_open_options_ext(&self) -> &FileSystem {
        self
    }

    /// Canonicalize a path without validating that it actually exists.
    pub fn canonicalize_unchecked(&self, path: &Path) -> Result<PathBuf> {
        let lock = self.inner.read().map_err(|_| FsError::Lock)?;
        lock.canonicalize_without_inode(path)
    }

    pub fn mount(
        &self,
        target_path: PathBuf,
        other: &Arc<dyn crate::FileSystem + Send + Sync>,
        source_path: PathBuf,
    ) -> Result<()> {
        if crate::FileSystem::read_dir(self, target_path.as_path()).is_ok() {
            return Err(FsError::AlreadyExists);
        }

        let (inode_of_parent, name_of_directory) = {
            // Read lock.
            let guard = self.inner.read().map_err(|_| FsError::Lock)?;

            // Canonicalize the path without checking the path exists,
            // because it's about to be created.
            let path = guard.canonicalize_without_inode(target_path.as_path())?;

            // Check the path has a parent.
            let parent_of_path = path.parent().ok_or(FsError::BaseNotDirectory)?;

            // Check the directory name.
            let name_of_directory = path
                .file_name()
                .ok_or(FsError::InvalidInput)?
                .to_os_string();

            // Find the parent inode.
            let inode_of_parent = match guard.inode_of_parent(parent_of_path)? {
                InodeResolution::Found(a) => a,
                InodeResolution::Redirect(..) => {
                    return Err(FsError::AlreadyExists);
                }
            };

            (inode_of_parent, name_of_directory)
        };

        {
            // Write lock.
            let mut fs = self.inner.write().map_err(|_| FsError::Lock)?;

            // Creating the directory in the storage.
            let inode_of_directory = fs.storage.vacant_entry().key();
            let real_inode_of_directory = fs.storage.insert(Node::ArcDirectory(ArcDirectoryNode {
                inode: inode_of_directory,
                parent_inode: inode_of_parent,
                name: name_of_directory,
                fs: other.clone(),
                path: source_path,
                metadata: {
                    let time = time();

                    Metadata {
                        ft: FileType::new_dir(),
                        accessed: time,
                        created: time,
                        modified: time,
                        len: 0,
                    }
                },
            }));

            assert_eq!(
                inode_of_directory, real_inode_of_directory,
                "new directory inode should have been correctly calculated",
            );

            // Adding the new directory to its parent.
            fs.add_child_to_node(inode_of_parent, inode_of_directory)?;
        }

        Ok(())
    }
}

impl crate::FileSystem for FileSystem {
    fn set_parent(&mut self, directory: Arc<dyn crate::Directory + Send + Sync>) -> Result<()> {
        let mut guard = self.inner.write().map_err(|_| FsError::Lock)?;
        guard.parent = Some(directory);
        Ok(())
    }
    fn parent(&self) -> Option<Arc<dyn crate::Directory + Send + Sync>> {
        let guard = self.inner.write().ok()?;
        guard.parent.clone()
    }

    fn as_dir(&self) -> Box<dyn crate::Directory + Send + Sync> {
        Box::new(Directory::new(ROOT_INODE, self.clone()))
    }

    fn read_dir(&self, path: &Path) -> Result<ReadDir> {
        // Read lock.
        let guard = self.inner.read().map_err(|_| FsError::Lock)?;

        // Canonicalize the path.
        let (_path, inode_of_directory) = guard.canonicalize(path)?;
        let inode_of_directory = match inode_of_directory {
            InodeResolution::Found(a) => a,
            InodeResolution::Redirect(fs, path) => {
                return fs.read_dir(path.as_path());
            }
        };
        guard.read_dir_inode(inode_of_directory)
    }

    fn create_dir(&self, path: &Path) -> Result<()> {
        if self.read_dir(path).is_ok() {
            return Err(FsError::AlreadyExists);
        }

        let (inode_of_parent, name_of_directory) = {
            // Read lock.
            let guard = self.inner.read().map_err(|_| FsError::Lock)?;

            // Canonicalize the path without checking the path exists,
            // because it's about to be created.
            let path = guard.canonicalize_without_inode(path)?;

            // Check the path has a parent.
            let parent_of_path = path.parent().ok_or(FsError::BaseNotDirectory)?;

            // Check the directory name.
            let name_of_directory = path
                .file_name()
                .ok_or(FsError::InvalidInput)?
                .to_os_string();

            // Find the parent inode.
            let inode_of_parent = match guard.inode_of_parent(parent_of_path)? {
                InodeResolution::Found(a) => a,
                InodeResolution::Redirect(fs, mut path) => {
                    drop(guard);
                    path.push(name_of_directory);
                    return fs.create_dir(path.as_path());
                }
            };

            (inode_of_parent, name_of_directory)
        };
        let mut guard = self.inner.write().map_err(|_| FsError::Lock)?;
        guard.create_dir_inode(inode_of_parent, name_of_directory)
    }

    fn remove_dir(&self, path: &Path) -> Result<()> {
        let mut guard = self.inner.write().map_err(|_| FsError::Lock)?;

        // Canonicalize the path.
        let (_path, node) = guard.canonicalize(path)?;

        let inode_of_directory = match node {
            InodeResolution::Found(a) => a,
            InodeResolution::Redirect(fs, path) => {
                return fs.remove_dir(path.as_path());
            }
        };

        guard.remove_dir_inode(inode_of_directory)
    }

    fn remove_file(&self, path: &Path) -> Result<()> {
        let mut guard = self.inner.write().map_err(|_| FsError::Lock)?;

        // Canonicalize the path.
        let (_path, node) = guard.canonicalize(path)?;

        let inode_of_file = match node {
            InodeResolution::Found(a) => a,
            InodeResolution::Redirect(fs, path) => {
                return fs.remove_file(path.as_path());
            }
        };

        guard.remove_file_inode(inode_of_file)
    }

    fn rename<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async {
            let name_of_to;

            let (
                (position_of_from, inode, inode_of_from_parent),
                (inode_of_to_parent, name_of_to),
                inode_dest,
            ) = {
                // Read lock.
                let fs = self.inner.read().map_err(|_| FsError::Lock)?;

                let from = fs.canonicalize_without_inode(from)?;
                let to = fs.canonicalize_without_inode(to)?;

                // Check the paths have parents.
                let parent_of_from = from.parent().ok_or(FsError::BaseNotDirectory)?;
                let parent_of_to = to.parent().ok_or(FsError::BaseNotDirectory)?;

                // Check the names.
                let name_of_from = from
                    .file_name()
                    .ok_or(FsError::InvalidInput)?
                    .to_os_string();
                name_of_to = to.file_name().ok_or(FsError::InvalidInput)?.to_os_string();

                // Find the parent inodes.
                let inode_of_from_parent = match fs.inode_of_parent(parent_of_from)? {
                    InodeResolution::Found(a) => a,
                    InodeResolution::Redirect(..) => {
                        return Err(FsError::InvalidInput);
                    }
                };
                let inode_of_to_parent = match fs.inode_of_parent(parent_of_to)? {
                    InodeResolution::Found(a) => a,
                    InodeResolution::Redirect(..) => {
                        return Err(FsError::InvalidInput);
                    }
                };

                // Find the inode of the dest file if it exists
                let maybe_position_and_inode_of_file =
                    fs.as_parent_get_position_and_inode_of_file(inode_of_to_parent, &name_of_to)?;

                // Get the child indexes to update in the parent nodes, in
                // addition to the inode of the directory to update.
                let (position_of_from, inode) = fs
                    .as_parent_get_position_and_inode(inode_of_from_parent, &name_of_from)?
                    .ok_or(FsError::EntryNotFound)?;

                (
                    (position_of_from, inode, inode_of_from_parent),
                    (inode_of_to_parent, name_of_to),
                    maybe_position_and_inode_of_file,
                )
            };

            let inode = match inode {
                InodeResolution::Found(a) => a,
                InodeResolution::Redirect(..) => {
                    return Err(FsError::InvalidInput);
                }
            };

            {
                // Write lock.
                let mut fs = self.inner.write().map_err(|_| FsError::Lock)?;

                if let Some((position, inode_of_file)) = inode_dest {
                    // Remove the file from the storage.
                    match inode_of_file {
                        InodeResolution::Found(inode_of_file) => {
                            fs.storage.remove(inode_of_file);
                        }
                        InodeResolution::Redirect(..) => {
                            return Err(FsError::InvalidInput);
                        }
                    }

                    fs.remove_child_from_node(inode_of_to_parent, position)?;
                }

                // Update the file name, and update the modified time.
                fs.update_node_name(inode, name_of_to)?;

                // The parents are different. Let's update them.
                if inode_of_from_parent != inode_of_to_parent {
                    // Remove the file from its parent, and update the
                    // modified time.
                    fs.remove_child_from_node(inode_of_from_parent, position_of_from)?;

                    // Add the file to its new parent, and update the modified
                    // time.
                    fs.add_child_to_node(inode_of_to_parent, inode)?;

                    // Replace the inode parent
                    let mut inode = fs.storage.get_mut(inode);
                    if let Some(inode_mut) = inode.as_mut() {
                        inode_mut.set_parent_inode(inode_of_to_parent);
                    };
                }
                // Otherwise, we need to at least update the modified time of the parent.
                else {
                    let mut inode = fs.storage.get_mut(inode_of_from_parent);
                    match inode.as_mut() {
                        Some(Node::Directory(node)) => node.metadata.modified = time(),
                        Some(Node::ArcDirectory(node)) => node.metadata.modified = time(),
                        _ => return Err(FsError::UnknownError),
                    }
                }
            }

            Ok(())
        })
    }

    fn metadata(&self, path: &Path) -> Result<Metadata> {
        // Read lock.
        let guard = self.inner.read().map_err(|_| FsError::Lock)?;
        match guard.inode_of(path)? {
            InodeResolution::Found(inode) => Ok(guard
                .storage
                .get(inode)
                .ok_or(FsError::UnknownError)?
                .metadata()
                .clone()),
            InodeResolution::Redirect(fs, path) => {
                drop(guard);
                fs.metadata(path.as_path())
            }
        }
    }

    fn new_open_options(&self) -> OpenOptions {
        OpenOptions::new(self)
    }
}

impl fmt::Debug for FileSystem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let fs: &FileSystemInner = &self.inner.read().unwrap();

        fs.fmt(formatter)
    }
}

/// The core of the file system. It contains a collection of `Node`s,
/// indexed by their respective `Inode` in a slab.
pub(super) struct FileSystemInner {
    pub(super) storage: Slab<Node>,
    pub(super) parent: Option<Arc<dyn crate::Directory + Send + Sync>>,
    pub(super) limiter: Option<crate::limiter::DynFsMemoryLimiter>,
}

#[derive(Debug)]
pub(super) enum InodeResolution {
    Found(Inode),
    Redirect(Arc<dyn crate::FileSystem + Send + Sync + 'static>, PathBuf),
}

impl InodeResolution {
    #[allow(dead_code)]
    pub fn unwrap(&self) -> Inode {
        match self {
            Self::Found(a) => *a,
            Self::Redirect(..) => {
                panic!("failed to unwrap the inode as the resolution is a redirect");
            }
        }
    }
}

impl FileSystemInner {
    pub(super) fn get_node(&self, inode: Inode) -> Option<&Node> {
        self.storage.get(inode)
    }

    pub(super) fn get_node_directory(&self, inode: Inode) -> Result<&DirectoryNode> {
        let node = self.storage.get(inode);
        match node {
            Some(Node::Directory(dir_node)) => Ok(dir_node),
            _ => Err(FsError::BaseNotDirectory),
        }
    }

    pub(super) fn get_node_directory_mut(&mut self, inode: Inode) -> Result<&mut DirectoryNode> {
        let node = self.storage.get_mut(inode);
        match node {
            Some(Node::Directory(dir_node)) => Ok(dir_node),
            _ => Err(FsError::BaseNotDirectory),
        }
    }

    #[inline]
    pub(super) fn absolute_path(&self, node: &Node) -> PathBuf {
        let parent = self.get_node(node.parent_inode()).unwrap();
        if parent.inode() == ROOT_INODE {
            if let Some(parent) = &self.parent {
                return parent.absolute_path().join(node.name());
            }
            return PathBuf::from("/").join(node.name());
        }
        self.absolute_path(parent).join(node.name())
    }

    fn read_dir_inode(&self, inode_of_directory: Inode) -> Result<ReadDir> {
        // Check it's a directory and fetch the immediate children as `DirEntry`.
        let inode = self
            .storage
            .get(inode_of_directory)
            .ok_or(FsError::InvalidInput)?;
        let base_path = self.absolute_path(inode);
        let children = match inode {
            Node::Directory(DirectoryNode { children, .. }) => children
                .iter()
                .filter_map(|inode| self.storage.get(*inode))
                .map(|node| DirEntry {
                    path: { base_path.join(node.name()) },
                    metadata: Ok(node.metadata().clone()),
                })
                .collect(),

            Node::ArcDirectory(ArcDirectoryNode { fs, path, .. }) => {
                return fs.read_dir(path.as_path());
            }

            _ => return Err(FsError::InvalidInput),
        };

        Ok(ReadDir::new(children))
    }

    fn create_dir_inode(&mut self, inode: Inode, dir_name: OsString) -> Result<()> {
        let node = self.get_node_directory(inode)?;
        if node.children.iter().any(|child_inode| {
            if let Some(node) = self.storage.get(*child_inode) {
                return node.name() == dir_name;
            }
            false
        }) {
            return Err(FsError::AlreadyExists);
        }

        // Creating the directory in the storage.
        let inode_of_directory = self.storage.vacant_entry().key();
        let real_inode_of_directory = self.storage.insert(Node::Directory(DirectoryNode {
            inode: inode_of_directory,
            parent_inode: inode,
            name: dir_name,
            children: Vec::new(),
            metadata: {
                let time = time();

                Metadata {
                    ft: FileType::new_dir(),
                    accessed: time,
                    created: time,
                    modified: time,
                    len: 0,
                }
            },
        }));

        assert_eq!(
            inode_of_directory, real_inode_of_directory,
            "new directory inode should have been correctly calculated",
        );

        // Adding the new directory to its parent.
        self.add_child_to_node(inode, inode_of_directory)?;

        Ok(())
    }

    fn remove_dir_inode(&mut self, inode: Inode) -> Result<()> {
        if inode == ROOT_INODE {
            // We can't remove the root
            return Err(FsError::BaseNotDirectory);
        }
        let node = self.get_node_directory(inode)?;
        if !node.children.is_empty() {
            return Err(FsError::DirectoryNotEmpty);
        }
        self.remove_inode_inside_dir(node.parent_inode, inode)
    }

    fn remove_inode_inside_dir(&mut self, inode: Inode, child: Inode) -> Result<()> {
        let mut parent_node = self.get_node_directory_mut(inode)?;
        let position = parent_node
            .children
            .iter()
            .position(|&r| r == child)
            .ok_or(FsError::EntryNotFound)?;

        parent_node.metadata.modified = time();

        // Remove the child from the parent directory.
        self.remove_child_from_node(inode, position)?;

        // Remove the directory from the storage.
        self.storage.remove(child);

        Ok(())
    }

    fn remove_file_inode(&mut self, inode: Inode) -> Result<()> {
        let node = self.get_node(inode).ok_or(FsError::EntryNotFound)?;
        self.remove_inode_inside_dir(node.parent_inode(), inode)
    }

    /// Get the inode associated to a path if it exists.
    pub(super) fn inode_of(&self, path: &Path) -> Result<InodeResolution> {
        // SAFETY: The root node always exists, so it's safe to unwrap here.
        let mut node = self.storage.get(ROOT_INODE).unwrap();
        let mut components = path.components();

        match components.next() {
            Some(Component::RootDir) => {}
            _ => return Err(FsError::BaseNotDirectory),
        }

        while let Some(component) = components.next() {
            node = match node {
                Node::Directory(DirectoryNode { children, .. }) => children
                    .iter()
                    .filter_map(|inode| self.storage.get(*inode))
                    .find(|node| node.name() == component.as_os_str())
                    .ok_or(FsError::EntryNotFound)?,
                Node::ArcDirectory(ArcDirectoryNode { fs, .. }) => {
                    let mut path = PathBuf::new();
                    path.push(PathBuf::from(component.as_os_str()));
                    for component in components.by_ref() {
                        path.push(PathBuf::from(component.as_os_str()));
                    }
                    // let mut path: PathBuf = components.collect();
                    // path.push(component.as_os_str());
                    return Ok(InodeResolution::Redirect(fs.clone(), path));
                }
                _ => return Err(FsError::BaseNotDirectory),
            };
        }

        Ok(InodeResolution::Found(node.inode()))
    }

    /// Get the inode associated to a “parent path”. The returned
    /// inode necessarily represents a directory.
    pub(super) fn inode_of_parent(&self, parent_path: &Path) -> Result<InodeResolution> {
        match self.inode_of(parent_path)? {
            InodeResolution::Found(inode_of_parent) => {
                // Ensure it is a directory.
                match self.storage.get(inode_of_parent) {
                    Some(Node::Directory(DirectoryNode { .. })) | Some(Node::ArcDirectory(_)) => {
                        Ok(InodeResolution::Found(inode_of_parent))
                    }
                    _ => Err(FsError::BaseNotDirectory),
                }
            }
            InodeResolution::Redirect(fs, path) => {
                println!("Inode of parent found -> Redirect: {}", path.display());
                Ok(InodeResolution::Redirect(fs, path))
            }
        }
    }

    /// From the inode of a parent node (so, a directory), returns the
    /// child index of `name_of_file` along with its inode.
    pub(super) fn as_parent_get_position_and_inode_of_file(
        &self,
        inode_of_parent: Inode,
        name_of_file: &OsString,
    ) -> Result<Option<(usize, InodeResolution)>> {
        match self.storage.get(inode_of_parent) {
            Some(Node::Directory(DirectoryNode { children, .. })) => children
                .iter()
                .enumerate()
                .filter_map(|(nth, inode)| self.storage.get(*inode).map(|node| (nth, node)))
                .find_map(|(nth, node)| match node {
                    Node::File(FileNode { inode, name, .. })
                    | Node::ReadOnlyFile(ReadOnlyFileNode { inode, name, .. })
                    | Node::CustomFile(CustomFileNode { inode, name, .. })
                        if name.as_os_str() == name_of_file =>
                    {
                        Some(Some((nth, InodeResolution::Found(*inode))))
                    }
                    _ => None,
                })
                .or(Some(None))
                .ok_or(FsError::InvalidInput),

            Some(Node::ArcDirectory(ArcDirectoryNode {
                fs, path: fs_path, ..
            })) => {
                let mut path = fs_path.clone();
                path.push(name_of_file);
                Ok(Some((0, InodeResolution::Redirect(fs.clone(), path))))
            }

            _ => Err(FsError::BaseNotDirectory),
        }
    }

    /// From the inode of a parent node (so, a directory), returns the
    /// child index of `name_of` along with its inode, whatever the
    /// type of inode is (directory or file).
    fn as_parent_get_position_and_inode(
        &self,
        inode_of_parent: Inode,
        name_of: &OsString,
    ) -> Result<Option<(usize, InodeResolution)>> {
        match self.storage.get(inode_of_parent) {
            Some(Node::Directory(DirectoryNode { children, .. })) => children
                .iter()
                .enumerate()
                .filter_map(|(nth, inode)| self.storage.get(*inode).map(|node| (nth, node)))
                .find_map(|(nth, node)| match node {
                    Node::File(FileNode { inode, name, .. })
                    | Node::Directory(DirectoryNode { inode, name, .. })
                    | Node::ReadOnlyFile(ReadOnlyFileNode { inode, name, .. })
                    | Node::CustomFile(CustomFileNode { inode, name, .. })
                        if name.as_os_str() == name_of =>
                    {
                        Some(Some((nth, InodeResolution::Found(*inode))))
                    }
                    _ => None,
                })
                .or(Some(None))
                .ok_or(FsError::InvalidInput),

            Some(Node::ArcDirectory(ArcDirectoryNode {
                fs, path: fs_path, ..
            })) => {
                let mut path = fs_path.clone();
                path.push(name_of);
                Ok(Some((0, InodeResolution::Redirect(fs.clone(), path))))
            }

            _ => Err(FsError::BaseNotDirectory),
        }
    }

    /// Set a new name for the node represented by `inode`.
    pub(super) fn update_node_name(&mut self, inode: Inode, new_name: OsString) -> Result<()> {
        let node = self.storage.get_mut(inode).ok_or(FsError::UnknownError)?;

        node.set_name(new_name);
        node.metadata_mut().modified = time();

        Ok(())
    }

    /// Add a child to a directory node represented by `inode`.
    ///
    /// This function also updates the modified time of the directory.
    ///
    /// # Safety
    ///
    /// `inode` must represents an existing directory.
    pub(super) fn add_child_to_node(&mut self, inode: Inode, new_child: Inode) -> Result<()> {
        match self.storage.get_mut(inode) {
            Some(Node::Directory(DirectoryNode {
                children,
                metadata: Metadata { modified, .. },
                ..
            })) => {
                children.push(new_child);
                *modified = time();

                Ok(())
            }
            _ => Err(FsError::UnknownError),
        }
    }

    /// Remove the child at position `position` of a directory node
    /// represented by `inode`.
    ///
    /// This function also updates the modified time of the directory.
    ///
    /// # Safety
    ///
    /// `inode` must represents an existing directory.
    pub(super) fn remove_child_from_node(&mut self, inode: Inode, position: usize) -> Result<()> {
        match self.storage.get_mut(inode) {
            Some(Node::Directory(DirectoryNode {
                children,
                metadata: Metadata { modified, .. },
                ..
            })) => {
                children.remove(position);
                *modified = time();

                Ok(())
            }
            _ => Err(FsError::UnknownError),
        }
    }

    /// Canonicalize a path, i.e. try to resolve to a canonical,
    /// absolute form of the path with all intermediate components
    /// normalized:
    ///
    /// * A path must starts with a root (`/`),
    /// * A path can contain `..` or `.` components,
    /// * A path must not contain a Windows prefix (`C:` or `\\server`),
    /// * A normalized path exists in the file system.
    pub(super) fn canonicalize(&self, path: &Path) -> Result<(PathBuf, InodeResolution)> {
        let new_path = self.canonicalize_without_inode(path)?;
        let inode = self.inode_of(&new_path)?;

        Ok((new_path, inode))
    }

    /// Like `Self::canonicalize` but without returning the inode of
    /// the path, which means that there is no guarantee that the path
    /// exists in the file system.
    pub(super) fn canonicalize_without_inode(&self, path: &Path) -> Result<PathBuf> {
        let mut components = path.components();

        match components.next() {
            Some(Component::RootDir) => {}
            _ => return Err(FsError::InvalidInput),
        }

        let mut new_path = PathBuf::with_capacity(path.as_os_str().len());
        new_path.push("/");

        for component in components {
            match component {
                // That's an error to get a `RootDir` a second time.
                Component::RootDir => return Err(FsError::UnknownError),

                // Nothing to do on `new_path`.
                Component::CurDir => (),

                // Pop the lastly inserted component on `new_path` if
                // any, otherwise it's an error.
                Component::ParentDir => {
                    if !new_path.pop() {
                        return Err(FsError::InvalidInput);
                    }
                }

                // A normal
                Component::Normal(name) => {
                    new_path.push(name);
                }

                // We don't support Windows path prefix.
                Component::Prefix(_) => return Err(FsError::InvalidInput),
            }
        }

        Ok(new_path)
    }
}

impl fmt::Debug for FileSystemInner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            formatter,
            "\n{inode:<8}    {ty:<4}    name",
            inode = "inode",
            ty = "type",
        )?;

        fn debug(
            nodes: Vec<&Node>,
            slf: &FileSystemInner,
            formatter: &mut fmt::Formatter<'_>,
            indentation: usize,
        ) -> fmt::Result {
            for node in nodes {
                writeln!(
                    formatter,
                    "{inode:<8}    {ty:<4}   {indentation_symbol:indentation_width$}{name}",
                    inode = node.inode(),
                    ty = match node {
                        Node::File { .. } => "file",
                        Node::ReadOnlyFile { .. } => "ro-file",
                        Node::CustomFile { .. } => "custom-file",
                        Node::Directory { .. } => "dir",
                        Node::ArcDirectory { .. } => "arc-dir",
                    },
                    name = node.name().to_string_lossy(),
                    indentation_symbol = " ",
                    indentation_width = indentation * 2 + 1,
                )?;

                if let Node::Directory(DirectoryNode { children, .. }) = node {
                    debug(
                        children
                            .iter()
                            .filter_map(|inode| slf.storage.get(*inode))
                            .collect(),
                        slf,
                        formatter,
                        indentation + 1,
                    )?;
                }
            }

            Ok(())
        }

        debug(
            vec![self.storage.get(ROOT_INODE).unwrap()],
            self,
            formatter,
            0,
        )
    }
}

impl Default for FileSystemInner {
    fn default() -> Self {
        let time = time();

        let mut slab = Slab::new();
        slab.insert(Node::Directory(DirectoryNode {
            inode: ROOT_INODE,
            // TODO: Fix this
            parent_inode: ROOT_INODE,
            name: OsString::from("/"),
            children: Vec::new(),
            metadata: Metadata {
                ft: FileType::new_dir(),
                accessed: time,
                created: time,
                modified: time,
                len: 0,
            },
        }));

        Self {
            storage: slab,
            parent: None,
            limiter: None,
        }
    }
}

#[cfg(test)]
mod test_filesystem {
    use std::{borrow::Cow, path::Path};

    use tokio::io::AsyncReadExt;

    use crate::{mem_fs::*, ops, DirEntry, FileSystem as FS, FileType, FsError};

    macro_rules! path {
        ($path:expr) => {
            std::path::Path::new($path)
        };

        (buf $path:expr) => {
            std::path::PathBuf::from($path)
        };
    }

    #[tokio::test]
    async fn test_new_filesystem() {
        let fs = FileSystem::default();
        let fs_inner = fs.inner.read().unwrap();

        assert_eq!(fs_inner.storage.len(), 1, "storage has a root");
        assert!(
            matches!(
                fs_inner.storage.get(ROOT_INODE),
                Some(Node::Directory(DirectoryNode {
                    inode: ROOT_INODE,
                    name,
                    children,
                    ..
                })) if name == "/" && children.is_empty(),
            ),
            "storage has a well-defined root",
        );
    }

    // #[tokio::test]
    // async fn test_create_dir_dot() {
    //     let fs = FileSystem::default();
    //     fs.create_dir(".").unwrap();

    //     assert_eq!(
    //         fs.create_dir(path!(".")),
    //         Err(FsError::AlreadyExists),
    //         "creating the root which already exists",
    //     );
    // }

    #[tokio::test]
    async fn test_create_dir() {
        let fs = FileSystem::default();

        assert_eq!(
            fs.create_dir(path!("/")),
            Err(FsError::AlreadyExists),
            "creating the root which already exists",
        );

        assert_eq!(fs.create_dir(path!("/foo")), Ok(()), "creating a directory",);

        {
            let fs_inner = fs.inner.read().unwrap();
            assert_eq!(
                fs_inner.storage.len(),
                2,
                "storage contains the new directory"
            );
            assert!(
                matches!(
                    fs_inner.storage.get(ROOT_INODE),
                    Some(Node::Directory(DirectoryNode {
                        inode: ROOT_INODE,
                        name,
                        children,
                        ..
                    })) if name == "/" && children == &[1]
                ),
                "the root is updated and well-defined",
            );
            assert!(
                matches!(
                    fs_inner.storage.get(1),
                    Some(Node::Directory(DirectoryNode {
                        inode: 1,
                        name,
                        children,
                        ..
                    })) if name == "foo" && children.is_empty(),
                ),
                "the new directory is well-defined",
            );
        }

        assert_eq!(
            fs.create_dir(path!("/foo/bar")),
            Ok(()),
            "creating a sub-directory",
        );

        {
            let fs_inner = fs.inner.read().unwrap();
            assert_eq!(
                fs_inner.storage.len(),
                3,
                "storage contains the new sub-directory",
            );
            assert!(
                matches!(
                    fs_inner.storage.get(ROOT_INODE),
                    Some(Node::Directory(DirectoryNode {
                        inode: ROOT_INODE,
                        name,
                        children,
                        ..
                    })) if name == "/" && children == &[1]
                ),
                "the root is updated again and well-defined",
            );
            assert!(
                matches!(
                    fs_inner.storage.get(1),
                    Some(Node::Directory(DirectoryNode {
                        inode: 1,
                        name,
                        children,
                        ..
                    })) if name == "foo" && children == &[2]
                ),
                "the new directory is updated and well-defined",
            );
            assert!(
                matches!(
                    fs_inner.storage.get(2),
                    Some(Node::Directory(DirectoryNode {
                        inode: 2,
                        name,
                        children,
                        ..
                    })) if name == "bar" && children.is_empty()
                ),
                "the new directory is well-defined",
            );
        }
    }

    #[tokio::test]
    async fn test_remove_dir() {
        let fs = FileSystem::default();

        assert_eq!(
            fs.remove_dir(path!("/")),
            Err(FsError::BaseNotDirectory),
            "removing a directory that has no parent",
        );

        assert_eq!(
            fs.remove_dir(path!("/foo")),
            Err(FsError::EntryNotFound),
            "cannot remove a directory that doesn't exist",
        );

        assert_eq!(fs.create_dir(path!("/foo")), Ok(()), "creating a directory",);

        assert_eq!(
            fs.create_dir(path!("/foo/bar")),
            Ok(()),
            "creating a sub-directory",
        );

        {
            let fs_inner = fs.inner.read().unwrap();
            assert_eq!(
                fs_inner.storage.len(),
                3,
                "storage contains all the directories",
            );
        }

        assert_eq!(
            fs.remove_dir(path!("/foo")),
            Err(FsError::DirectoryNotEmpty),
            "removing a directory that has children",
        );

        assert_eq!(
            fs.remove_dir(path!("/foo/bar")),
            Ok(()),
            "removing a sub-directory",
        );

        assert_eq!(fs.remove_dir(path!("/foo")), Ok(()), "removing a directory",);

        {
            let fs_inner = fs.inner.read().unwrap();
            assert_eq!(
                fs_inner.storage.len(),
                1,
                "storage contains all the directories",
            );
        }
    }

    #[tokio::test]
    async fn test_rename() {
        let fs = FileSystem::default();

        assert_eq!(
            fs.rename(path!("/"), path!("/bar")).await,
            Err(FsError::BaseNotDirectory),
            "renaming a directory that has no parent",
        );
        assert_eq!(
            fs.rename(path!("/foo"), path!("/")).await,
            Err(FsError::BaseNotDirectory),
            "renaming to a directory that has no parent",
        );

        assert_eq!(fs.create_dir(path!("/foo")), Ok(()));
        assert_eq!(fs.create_dir(path!("/foo/qux")), Ok(()));

        assert_eq!(
            fs.rename(path!("/foo"), path!("/bar/baz")).await,
            Err(FsError::EntryNotFound),
            "renaming to a directory that has parent that doesn't exist",
        );

        assert_eq!(fs.create_dir(path!("/bar")), Ok(()));

        assert!(
            matches!(
                fs.new_open_options()
                    .write(true)
                    .create_new(true)
                    .open(path!("/bar/hello1.txt")),
                Ok(_),
            ),
            "creating a new file (`hello1.txt`)",
        );
        assert!(
            matches!(
                fs.new_open_options()
                    .write(true)
                    .create_new(true)
                    .open(path!("/bar/hello2.txt")),
                Ok(_),
            ),
            "creating a new file (`hello2.txt`)",
        );

        {
            let fs_inner = fs.inner.read().unwrap();

            assert_eq!(fs_inner.storage.len(), 6, "storage has all files");
            assert!(
                matches!(
                    fs_inner.storage.get(ROOT_INODE),
                    Some(Node::Directory(DirectoryNode {
                        inode: ROOT_INODE,
                        name,
                        children,
                        ..
                    })) if name == "/" && children == &[1, 3]
                ),
                "`/` contains `foo` and `bar`",
            );
            assert!(
                matches!(
                    fs_inner.storage.get(1),
                    Some(Node::Directory(DirectoryNode {
                        inode: 1,
                        name,
                        children,
                        ..
                    })) if name == "foo" && children == &[2]
                ),
                "`foo` contains `qux`",
            );
            assert!(
                matches!(
                    fs_inner.storage.get(2),
                    Some(Node::Directory(DirectoryNode {
                        parent_inode: 1,
                        inode: 2,
                        name,
                        children,
                        ..
                    })) if name == "qux" && children.is_empty()
                ),
                "`qux` is empty",
            );
            assert!(
                matches!(
                    fs_inner.storage.get(3),
                    Some(Node::Directory(DirectoryNode {
                        inode: 3,
                        parent_inode: ROOT_INODE,
                        name,
                        children,
                        ..
                    })) if name == "bar" && children == &[4, 5]
                ),
                "`bar` is contains `hello.txt`",
            );
            assert!(
                matches!(
                    fs_inner.storage.get(4),
                    Some(Node::File(FileNode {
                        inode: 4,
                        parent_inode: 3,
                        name,
                        ..
                    })) if name == "hello1.txt"
                ),
                "`hello1.txt` exists",
            );
            assert!(
                matches!(
                    fs_inner.storage.get(5),
                    Some(Node::File(FileNode {
                        inode: 5,
                        parent_inode: 3,
                        name,
                        ..
                    })) if name == "hello2.txt"
                ),
                "`hello2.txt` exists",
            );
        }

        assert_eq!(
            fs.rename(path!("/bar/hello2.txt"), path!("/foo/world2.txt"))
                .await,
            Ok(()),
            "renaming (and moving) a file",
        );

        assert_eq!(
            fs.rename(path!("/foo"), path!("/bar/baz")).await,
            Ok(()),
            "renaming a directory",
        );

        assert_eq!(
            fs.rename(path!("/bar/hello1.txt"), path!("/bar/world1.txt"))
                .await,
            Ok(()),
            "renaming a file (in the same directory)",
        );

        {
            let fs_inner = fs.inner.read().unwrap();

            dbg!(&fs_inner);

            assert_eq!(
                fs_inner.storage.len(),
                6,
                "storage has still all directories"
            );
            assert!(
                matches!(
                    fs_inner.storage.get(ROOT_INODE),
                    Some(Node::Directory(DirectoryNode {
                        inode: ROOT_INODE,
                        name,
                        children,
                        ..
                    })) if name == "/" && children == &[3]
                ),
                "`/` contains `bar`",
            );
            assert!(
                matches!(
                    dbg!(fs_inner.storage.get(1)),
                    Some(Node::Directory(DirectoryNode {
                        parent_inode: 3,
                        inode: 1,
                        name,
                        children,
                        ..
                    })) if name == "baz" && children == &[2, 5]
                ),
                "`foo` has been renamed to `baz` and contains `qux` and `world2.txt`",
            );
            assert!(
                matches!(
                    fs_inner.storage.get(2),
                    Some(Node::Directory(DirectoryNode {
                        parent_inode: 1,
                        inode: 2,
                        name,
                        children,
                        ..
                    })) if name == "qux" && children.is_empty()
                ),
                "`qux` is empty",
            );
            assert!(
                matches!(
                    fs_inner.storage.get(3),
                    Some(Node::Directory(DirectoryNode {
                        parent_inode: ROOT_INODE,
                        inode: 3,
                        name,
                        children,
                        ..
                    })) if name == "bar" && children == &[4, 1]
                ),
                "`bar` contains `bar` (ex `foo`)  and `world1.txt` (ex `hello1`)",
            );
            assert!(
                matches!(
                    fs_inner.storage.get(4),
                    Some(Node::File(FileNode {
                        inode: 4,
                        name,
                        ..
                    })) if name == "world1.txt"
                ),
                "`hello1.txt` has been renamed to `world1.txt`",
            );
            assert!(
                matches!(
                    dbg!(fs_inner.storage.get(5)),
                    Some(Node::File(FileNode {
                        inode: 5,
                        parent_inode: 1,
                        name,
                        ..
                    })) if name == "world2.txt"
                ),
                "`hello2.txt` has been renamed to `world2.txt`",
            );
        }
    }

    #[tokio::test]
    async fn test_metadata() {
        use std::thread::sleep;
        use std::time::Duration;

        let fs = FileSystem::default();
        let root_metadata = fs.metadata(path!("/"));

        assert!(matches!(
            root_metadata,
            Ok(Metadata {
                ft: FileType { dir: true, .. },
                accessed,
                created,
                modified,
                len: 0
            }) if accessed == created && created == modified && modified > 0
        ));

        assert_eq!(fs.create_dir(path!("/foo")), Ok(()));

        let foo_metadata = fs.metadata(path!("/foo"));
        assert!(foo_metadata.is_ok());
        let foo_metadata = foo_metadata.unwrap();

        assert!(matches!(
            foo_metadata,
            Metadata {
                ft: FileType { dir: true, .. },
                accessed,
                created,
                modified,
                len: 0
            } if accessed == created && created == modified && modified > 0
        ));

        sleep(Duration::from_secs(3));

        assert_eq!(fs.rename(path!("/foo"), path!("/bar")).await, Ok(()));

        assert!(
            matches!(
                fs.metadata(path!("/bar")),
                Ok(Metadata {
                    ft: FileType { dir: true, .. },
                    accessed,
                    created,
                    modified,
                    len: 0
                }) if
                    accessed == foo_metadata.accessed &&
                    created == foo_metadata.created &&
                    modified > foo_metadata.modified
            ),
            "the modified time is updated when file is renamed",
        );
        assert!(
            matches!(
                fs.metadata(path!("/")),
                Ok(Metadata {
                    ft: FileType { dir: true, .. },
                    accessed,
                    created,
                    modified,
                    len: 0
                }) if
                    accessed == foo_metadata.accessed &&
                    created == foo_metadata.created &&
                    modified > foo_metadata.modified
            ),
            "the modified time of the parent is updated when file is renamed",
        );
    }

    #[tokio::test]
    async fn test_remove_file() {
        let fs = FileSystem::default();

        assert!(
            matches!(
                fs.new_open_options()
                    .write(true)
                    .create_new(true)
                    .open(path!("/foo.txt")),
                Ok(_)
            ),
            "creating a new file",
        );

        {
            let fs_inner = fs.inner.read().unwrap();

            assert_eq!(fs_inner.storage.len(), 2, "storage has all files");
            assert!(
                matches!(
                    fs_inner.storage.get(ROOT_INODE),
                    Some(Node::Directory(DirectoryNode {
                        inode: ROOT_INODE,
                        name,
                        children,
                        ..
                    })) if name == "/" && children == &[1]
                ),
                "`/` contains `foo.txt`",
            );
            assert!(
                matches!(
                    fs_inner.storage.get(1),
                    Some(Node::File(FileNode {
                        inode: 1,
                        name,
                        ..
                    })) if name == "foo.txt"
                ),
                "`foo.txt` exists and is a file",
            );
        }

        assert_eq!(
            fs.remove_file(path!("/foo.txt")),
            Ok(()),
            "removing a file that exists",
        );

        {
            let fs_inner = fs.inner.read().unwrap();

            assert_eq!(fs_inner.storage.len(), 1, "storage no longer has the file");
            assert!(
                matches!(
                    fs_inner.storage.get(ROOT_INODE),
                    Some(Node::Directory(DirectoryNode {
                        inode: ROOT_INODE,
                        name,
                        children,
                        ..
                    })) if name == "/" && children.is_empty()
                ),
                "`/` is empty",
            );
        }

        assert_eq!(
            fs.remove_file(path!("/foo.txt")),
            Err(FsError::EntryNotFound),
            "removing a file that exists",
        );
    }

    #[tokio::test]
    async fn test_readdir() {
        let fs = FileSystem::default();

        assert_eq!(fs.create_dir(path!("/foo")), Ok(()), "creating `foo`");
        assert_eq!(fs.create_dir(path!("/foo/sub")), Ok(()), "creating `sub`");
        assert_eq!(fs.create_dir(path!("/bar")), Ok(()), "creating `bar`");
        assert_eq!(fs.create_dir(path!("/baz")), Ok(()), "creating `bar`");
        assert!(
            matches!(
                fs.new_open_options()
                    .write(true)
                    .create_new(true)
                    .open(path!("/a.txt")),
                Ok(_)
            ),
            "creating `a.txt`",
        );
        assert!(
            matches!(
                fs.new_open_options()
                    .write(true)
                    .create_new(true)
                    .open(path!("/b.txt")),
                Ok(_)
            ),
            "creating `b.txt`",
        );

        let readdir = fs.read_dir(path!("/"));

        assert!(readdir.is_ok(), "reading the directory `/`");

        let mut readdir = readdir.unwrap();

        assert!(
            matches!(
                readdir.next(),
                Some(Ok(DirEntry {
                    path,
                    metadata: Ok(Metadata { ft, .. }),
                }))
                    if path == path!(buf "/foo") && ft.is_dir()
            ),
            "checking entry #1",
        );
        assert!(
            matches!(
                readdir.next(),
                Some(Ok(DirEntry {
                    path,
                    metadata: Ok(Metadata { ft, .. }),
                }))
                    if path == path!(buf "/bar") && ft.is_dir()
            ),
            "checking entry #2",
        );
        assert!(
            matches!(
                readdir.next(),
                Some(Ok(DirEntry {
                    path,
                    metadata: Ok(Metadata { ft, .. }),
                }))
                    if path == path!(buf "/baz") && ft.is_dir()
            ),
            "checking entry #3",
        );
        assert!(
            matches!(
                readdir.next(),
                Some(Ok(DirEntry {
                    path,
                    metadata: Ok(Metadata { ft, .. }),
                }))
                    if path == path!(buf "/a.txt") && ft.is_file()
            ),
            "checking entry #4",
        );
        assert!(
            matches!(
                readdir.next(),
                Some(Ok(DirEntry {
                    path,
                    metadata: Ok(Metadata { ft, .. }),
                }))
                    if path == path!(buf "/b.txt") && ft.is_file()
            ),
            "checking entry #5",
        );
        assert!(matches!(readdir.next(), None), "no more entries");
    }

    #[tokio::test]
    async fn test_canonicalize() {
        let fs = FileSystem::default();

        assert_eq!(fs.create_dir(path!("/foo")), Ok(()), "creating `foo`");
        assert_eq!(fs.create_dir(path!("/foo/bar")), Ok(()), "creating `bar`");
        assert_eq!(
            fs.create_dir(path!("/foo/bar/baz")),
            Ok(()),
            "creating `baz`",
        );
        assert_eq!(
            fs.create_dir(path!("/foo/bar/baz/qux")),
            Ok(()),
            "creating `qux`",
        );
        assert!(
            matches!(
                fs.new_open_options()
                    .write(true)
                    .create_new(true)
                    .open(path!("/foo/bar/baz/qux/hello.txt")),
                Ok(_)
            ),
            "creating `hello.txt`",
        );

        let fs_inner = fs.inner.read().unwrap();

        assert_eq!(
            fs_inner
                .canonicalize(path!("/"))
                .map(|(a, b)| (a, b.unwrap())),
            Ok((path!(buf "/"), ROOT_INODE)),
            "canonicalizing `/`",
        );
        assert_eq!(
            fs_inner
                .canonicalize(path!("foo"))
                .map(|(a, b)| (a, b.unwrap())),
            Err(FsError::InvalidInput),
            "canonicalizing `foo`",
        );
        assert_eq!(
            fs_inner
                .canonicalize(path!("/././././foo/"))
                .map(|(a, b)| (a, b.unwrap())),
            Ok((path!(buf "/foo"), 1)),
            "canonicalizing `/././././foo/`",
        );
        assert_eq!(
            fs_inner
                .canonicalize(path!("/foo/bar//"))
                .map(|(a, b)| (a, b.unwrap())),
            Ok((path!(buf "/foo/bar"), 2)),
            "canonicalizing `/foo/bar//`",
        );
        assert_eq!(
            fs_inner
                .canonicalize(path!("/foo/bar/../bar"))
                .map(|(a, b)| (a, b.unwrap())),
            Ok((path!(buf "/foo/bar"), 2)),
            "canonicalizing `/foo/bar/../bar`",
        );
        assert_eq!(
            fs_inner
                .canonicalize(path!("/foo/bar/../.."))
                .map(|(a, b)| (a, b.unwrap())),
            Ok((path!(buf "/"), ROOT_INODE)),
            "canonicalizing `/foo/bar/../..`",
        );
        assert_eq!(
            fs_inner
                .canonicalize(path!("/foo/bar/../../.."))
                .map(|(a, b)| (a, b.unwrap())),
            Err(FsError::InvalidInput),
            "canonicalizing `/foo/bar/../../..`",
        );
        assert_eq!(
            fs_inner
                .canonicalize(path!("C:/foo/"))
                .map(|(a, b)| (a, b.unwrap())),
            Err(FsError::InvalidInput),
            "canonicalizing `C:/foo/`",
        );
        assert_eq!(
            fs_inner
                .canonicalize(path!(
                    "/foo/./../foo/bar/../../foo/bar/./baz/./../baz/qux/../../baz/./qux/hello.txt"
                ))
                .map(|(a, b)| (a, b.unwrap())),
            Ok((path!(buf "/foo/bar/baz/qux/hello.txt"), 5)),
            "canonicalizing a crazily stupid path name",
        );
    }

    #[tokio::test]
    #[ignore = "Not yet supported. See https://github.com/wasmerio/wasmer/issues/3678"]
    async fn mount_to_overlapping_directories() {
        let top_level = FileSystem::default();
        ops::touch(&top_level, "/file.txt").unwrap();
        let nested = FileSystem::default();
        ops::touch(&nested, "/another-file.txt").unwrap();
        let top_level: Arc<dyn crate::FileSystem + Send + Sync> = Arc::new(top_level);
        let nested: Arc<dyn crate::FileSystem + Send + Sync> = Arc::new(nested);

        let fs = FileSystem::default();
        fs.mount("/top-level".into(), &top_level, "/".into())
            .unwrap();
        fs.mount("/top-level/nested".into(), &nested, "/".into())
            .unwrap();

        assert!(ops::is_dir(&fs, "/top-level"));
        assert!(ops::is_file(&fs, "/top-level/file.txt"));
        assert!(ops::is_dir(&fs, "/top-level/nested"));
        assert!(ops::is_file(&fs, "/top-level/nested/another-file.txt"));
    }

    #[tokio::test]
    async fn test_merge_flat() {
        let main = FileSystem::default();

        let other = FileSystem::default();
        crate::ops::create_dir_all(&other, "/a/x").unwrap();
        other
            .insert_ro_file(&Path::new("/a/x/a.txt"), Cow::Borrowed(b"a"))
            .unwrap();
        other
            .insert_ro_file(&Path::new("/a/x/b.txt"), Cow::Borrowed(b"b"))
            .unwrap();
        other
            .insert_ro_file(&Path::new("/a/x/c.txt"), Cow::Borrowed(b"c"))
            .unwrap();

        let out = other.read_dir(&Path::new("/")).unwrap();
        dbg!(&out);

        let other: Arc<dyn crate::FileSystem + Send + Sync> = Arc::new(other);

        let mut buf = Vec::new();

        main.mount("/x".into(), &other, "/a/x".into()).unwrap();
        let mut f = main
            .new_open_options()
            .read(true)
            .open(&Path::new("/x/a.txt"))
            .unwrap();
        f.read_to_end(&mut buf).await.unwrap();

        assert_eq!(buf, b"a");
    }
}
