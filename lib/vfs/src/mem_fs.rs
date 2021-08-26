use crate::{
    FileDescriptor, FileType, FsError, Metadata, OpenOptions, OpenOptionsConfig, ReadDir, Result,
    VirtualFile,
};
#[cfg(feature = "enable-serde")]
use serde::{Deserialize, Serialize};
use slab::Slab;
use std::cmp;
use std::convert::{identity, TryInto};
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io::{self, Read, Seek, Write};
use std::path::Path;
use std::str;
use std::sync::{Arc, RwLock, RwLockReadGuard};
use std::time::SystemTime;
use tracing::debug;

pub type Inode = usize;
const ROOT_INODE: Inode = 0;

#[derive(Debug)]
enum Node {
    File {
        inode: Inode,
        name: OsString,
        file: File,
        metadata: Metadata,
    },
    Directory {
        inode: Inode,
        name: OsString,
        children: Vec<Inode>,
        metadata: Metadata,
    },
}

impl Node {
    fn inode(&self) -> Inode {
        *match self {
            Self::File { inode, .. } => inode,
            Self::Directory { inode, .. } => inode,
        }
    }

    fn name(&self) -> &OsStr {
        match self {
            Self::File { name, .. } => name.as_os_str(),
            Self::Directory { name, .. } => name.as_os_str(),
        }
    }

    fn metadata(&self) -> &Metadata {
        match self {
            Self::File { metadata, .. } => metadata,
            Self::Directory { metadata, .. } => metadata,
        }
    }

    fn metadata_mut(&mut self) -> &mut Metadata {
        match self {
            Self::File { metadata, .. } => metadata,
            Self::Directory { metadata, .. } => metadata,
        }
    }

    fn set_name(&mut self, new_name: OsString) {
        match self {
            Self::File { name, .. } => *name = new_name,
            Self::Directory { name, .. } => *name = new_name,
        }
    }
}

enum DirectoryMustBeEmpty {
    Yes,
    No,
}

impl DirectoryMustBeEmpty {
    fn yes(&self) -> bool {
        matches!(self, Self::Yes)
    }

    fn no(&self) -> bool {
        !self.yes()
    }
}

#[derive(Clone, Default)]
pub struct FileSystem {
    inner: Arc<RwLock<FileSystemInner>>,
}

impl crate::FileSystem for FileSystem {
    fn read_dir(&self, _path: &Path) -> Result<ReadDir> {
        todo!()
    }

    fn create_dir(&self, path: &Path) -> Result<()> {
        let (inode_of_parent, name_of_directory) = {
            // Read lock.
            let fs = self.inner.try_read().map_err(|_| FsError::Lock)?;

            // Check the path has a parent.
            let parent_of_path = path.parent().ok_or(FsError::BaseNotDirectory)?;

            // Check the directory name.
            let name_of_directory = path
                .file_name()
                .ok_or(FsError::InvalidInput)?
                .to_os_string();

            // Find the parent node.
            let inode_of_parent = fs.inode_of_parent(parent_of_path)?;

            (inode_of_parent, name_of_directory)
        };

        {
            // Write lock.
            let mut fs = self.inner.try_write().map_err(|_| FsError::Lock)?;

            // Creating the directory in the storage.
            let inode_of_directory = fs.storage.vacant_entry().key();
            let real_inode_of_directory = fs.storage.insert(Node::Directory {
                inode: inode_of_directory,
                name: name_of_directory,
                children: Vec::new(),
                metadata: {
                    let time = time();

                    Metadata {
                        ft: FileType {
                            dir: true,
                            ..Default::default()
                        },
                        accessed: time,
                        created: time,
                        modified: time,
                        len: 0,
                    }
                },
            });

            assert_eq!(
                inode_of_directory, real_inode_of_directory,
                "new directory inode should have been correctly calculated",
            );

            // Adding the new directory to its parent.
            fs.add_child_to_node(inode_of_parent, inode_of_directory)?;
        }

        Ok(())
    }

    fn remove_dir(&self, path: &Path) -> Result<()> {
        let (inode_of_parent, position, inode_of_directory) = {
            // Read lock.
            let fs = self.inner.try_read().map_err(|_| FsError::Lock)?;

            // Check the path has a parent.
            let parent_of_path = path.parent().ok_or(FsError::BaseNotDirectory)?;

            // Check the directory name.
            let name_of_directory = path
                .file_name()
                .ok_or(FsError::InvalidInput)?
                .to_os_string();

            // Find the parent node.
            let inode_of_parent = fs.inode_of_parent(parent_of_path)?;

            // Get the child index to remove in the parent node, in
            // addition to the inode of the directory to remove.
            let (position, inode_of_directory) = fs
                .from_parent_get_position_and_inode_of_directory(
                    inode_of_parent,
                    &name_of_directory,
                    DirectoryMustBeEmpty::Yes,
                )?;

            (inode_of_parent, position, inode_of_directory)
        };

        {
            // Write lock.
            let mut fs = self.inner.try_write().map_err(|_| FsError::Lock)?;

            // Remove the directory from the storage.
            fs.storage.remove(inode_of_directory);

            // Remove the child from the parent directory.
            fs.remove_child_from_node(inode_of_parent, position)?;
        }

        Ok(())
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        let (
            (position_of_from, inode, inode_of_from_parent),
            (inode_of_to_parent, name_of_to_directory),
        ) = {
            // Read lock.
            let fs = self.inner.try_read().map_err(|_| FsError::Lock)?;

            // Check the paths have parents.
            let parent_of_from = from.parent().ok_or(FsError::BaseNotDirectory)?;
            let parent_of_to = to.parent().ok_or(FsError::BaseNotDirectory)?;

            // Check the directory names.
            let name_of_from_directory = from
                .file_name()
                .ok_or(FsError::InvalidInput)?
                .to_os_string();
            let name_of_to_directory = to.file_name().ok_or(FsError::InvalidInput)?.to_os_string();

            // Find the parent nodes.
            let inode_of_from_parent = fs.inode_of_parent(parent_of_from)?;
            let inode_of_to_parent = fs.inode_of_parent(parent_of_to)?;

            // Get the child indexes to update in the parent nodes, in
            // addition to the inode of the directory to update.
            let (position_of_from, inode) = fs.from_parent_get_position_and_inode_of_directory(
                inode_of_from_parent,
                &name_of_from_directory,
                DirectoryMustBeEmpty::No,
            )?;

            (
                (position_of_from, inode, inode_of_from_parent),
                (inode_of_to_parent, name_of_to_directory),
            )
        };

        {
            // Write lock.
            let mut fs = self.inner.try_write().map_err(|_| FsError::Lock)?;

            // Update the directory name, and update the modified
            // time.
            fs.update_node_name(inode, name_of_to_directory)?;

            // Remove the directory from its parent, and update the
            // modified time.
            fs.remove_child_from_node(inode_of_from_parent, position_of_from);

            // Add the directory to its new parent, and update the
            // modified time.
            fs.add_child_to_node(inode_of_to_parent, inode);
        }

        Ok(())
    }

    fn metadata(&self, path: &Path) -> Result<Metadata> {
        // Read lock.
        let fs = self.inner.try_read().map_err(|_| FsError::Lock)?;

        Ok(fs
            .storage
            .get(fs.inode_of(path)?)
            .ok_or(FsError::UnknownError)?
            .metadata()
            .clone())
    }

    fn remove_file(&self, path: &Path) -> Result<()> {
        let (inode_of_parent, position, inode_of_file) = {
            // Read lock.
            let fs = self.inner.try_read().map_err(|_| FsError::Lock)?;

            // Check the path has a parent.
            let parent_of_path = path.parent().ok_or(FsError::BaseNotDirectory)?;

            // Check the file name.
            let name_of_file = path
                .file_name()
                .ok_or(FsError::InvalidInput)?
                .to_os_string();

            // Find the parent node.
            let inode_of_parent = fs.inode_of_parent(parent_of_path)?;

            // Find the inode of the file if it exists, along with its position.
            let maybe_position_and_inode_of_file =
                fs.from_parent_get_position_and_inode_of_file(inode_of_parent, &name_of_file)?;

            match maybe_position_and_inode_of_file {
                Some((position, inode_of_file)) => (inode_of_parent, position, inode_of_file),
                None => return Err(FsError::NotAFile),
            }
        };

        {
            // Write lock.
            let mut fs = self.inner.try_write().map_err(|_| FsError::Lock)?;

            // Remove the file from the storage.
            fs.storage.remove(inode_of_file);

            // Remove the child from the parent directory.
            fs.remove_child_from_node(inode_of_parent, position)?;
        }

        Ok(())
    }

    fn new_open_options(&self) -> OpenOptions {
        OpenOptions::new(Box::new(FileOpener {
            filesystem: self.clone(),
        }))
    }
}

impl fmt::Debug for FileSystem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let fs: &FileSystemInner = &self.inner.read().unwrap();

        fs.fmt(formatter)
    }
}

struct FileSystemInner {
    storage: Slab<Node>,
}

impl FileSystemInner {
    /// Get the inode associated to a path if it exists.
    fn inode_of(&self, path: &Path) -> Result<Inode> {
        // SAFETY: The root node always exists, so it's safe to unwrap here.
        let mut node = self.storage.get(ROOT_INODE).unwrap();
        let mut components = path.components();

        match components.next() {
            Some(component) if node.name() == component.as_os_str() => {}
            _ => return Err(FsError::BaseNotDirectory),
        }

        for component in components {
            node = match node {
                Node::Directory { children, .. } => children
                    .iter()
                    .filter_map(|inode| self.storage.get(*inode))
                    .find_map(|node| {
                        if node.name() == component.as_os_str() {
                            Some(node)
                        } else {
                            None
                        }
                    })
                    .ok_or(FsError::NotAFile)?,
                _ => return Err(FsError::BaseNotDirectory),
            };
        }

        Ok(node.inode())
    }

    /// Get the inode associated to a “parent path”. The returned
    /// inode necessarily represents a directory.
    fn inode_of_parent(&self, parent_path: &Path) -> Result<Inode> {
        let inode_of_parent = self.inode_of(parent_path)?;

        // Ensure it is a directory.
        match self.storage.get(inode_of_parent) {
            Some(Node::Directory { .. }) => Ok(inode_of_parent),
            _ => Err(FsError::BaseNotDirectory),
        }
    }

    /// From the inode of a parent node (so, a directory), returns the
    /// child index of `name_of_directory` along with its inode.
    fn from_parent_get_position_and_inode_of_directory(
        &self,
        inode_of_parent: Inode,
        name_of_directory: &OsString,
        directory_must_be_empty: DirectoryMustBeEmpty,
    ) -> Result<(usize, Inode)> {
        match self.storage.get(inode_of_parent) {
            Some(Node::Directory { children, .. }) => children
                .iter()
                .enumerate()
                .filter_map(|(nth, inode)| self.storage.get(*inode).map(|node| (nth, node)))
                .find_map(|(nth, node)| match node {
                    Node::Directory {
                        inode,
                        name,
                        children,
                        ..
                    } if name.as_os_str() == name_of_directory => {
                        if directory_must_be_empty.no() || children.is_empty() {
                            Some(Ok((nth, *inode)))
                        } else {
                            Some(Err(FsError::DirectoryNotEmpty))
                        }
                    }

                    _ => None,
                })
                .ok_or(FsError::InvalidInput)
                .and_then(identity), // flatten
            _ => Err(FsError::BaseNotDirectory),
        }
    }

    /// From the inode of a parent node (so, a directory), returns the
    /// child index of `name_of_file` along with its inode.
    fn from_parent_get_position_and_inode_of_file(
        &self,
        inode_of_parent: Inode,
        name_of_file: &OsString,
    ) -> Result<Option<(usize, Inode)>> {
        match self.storage.get(inode_of_parent) {
            Some(Node::Directory { children, .. }) => children
                .iter()
                .enumerate()
                .filter_map(|(nth, inode)| self.storage.get(*inode).map(|node| (nth, node)))
                .find_map(|(nth, node)| match node {
                    Node::File { inode, name, .. } if name.as_os_str() == name_of_file => {
                        Some(Some((nth, *inode)))
                    }

                    _ => None,
                })
                .or_else(|| Some(None))
                .ok_or(FsError::InvalidInput),

            _ => Err(FsError::BaseNotDirectory),
        }
    }

    /// Set a new name for the node represented by `inode`.
    fn update_node_name(&mut self, inode: Inode, new_name: OsString) -> Result<()> {
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
    fn add_child_to_node(&mut self, inode: Inode, new_child: Inode) -> Result<()> {
        match self.storage.get_mut(inode) {
            Some(Node::Directory {
                children,
                metadata: Metadata { modified, .. },
                ..
            }) => {
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
    fn remove_child_from_node(&mut self, inode: Inode, position: usize) -> Result<()> {
        match self.storage.get_mut(inode) {
            Some(Node::Directory {
                children,
                metadata: Metadata { modified, .. },
                ..
            }) => {
                children.remove(position);
                *modified = time();

                Ok(())
            }
            _ => Err(FsError::UnknownError),
        }
    }
}

impl fmt::Debug for FileSystemInner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "\n{inode:<8}    {ty:<4}    {name}\n",
            inode = "inode",
            ty = "type",
            name = "name",
        )?;

        fn debug(
            nodes: Vec<&Node>,
            slf: &FileSystemInner,
            formatter: &mut fmt::Formatter<'_>,
            indentation: usize,
        ) -> fmt::Result {
            for node in nodes {
                write!(
                    formatter,
                    "{inode:<8}    {ty:<4}   {indentation_symbol:indentation_width$}{name}\n",
                    inode = node.inode(),
                    ty = match node {
                        Node::File { .. } => "file",
                        Node::Directory { .. } => "dir",
                    },
                    name = node.name().to_string_lossy(),
                    indentation_symbol = " ",
                    indentation_width = indentation * 2 + 1,
                )?;

                if let Node::Directory { children, .. } = node {
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
            &self,
            formatter,
            0,
        )
    }
}

impl Default for FileSystemInner {
    fn default() -> Self {
        let time = time();

        let mut slab = Slab::new();
        slab.insert(Node::Directory {
            inode: ROOT_INODE,
            name: OsString::from("/"),
            children: Vec::new(),
            metadata: Metadata {
                ft: FileType {
                    dir: true,
                    ..Default::default()
                },
                accessed: time,
                created: time,
                modified: time,
                len: 0,
            },
        });

        Self { storage: slab }
    }
}

#[cfg(test)]
macro_rules! path {
    ($path:expr) => {
        Path::new($path)
    };
}

#[cfg(test)]
mod test_filesystem {
    use super::*;
    use crate::FileSystem as FS;

    #[test]
    fn test_new_filesystem() {
        let fs = FileSystem::default();
        let fs_inner = fs.inner.read().unwrap();

        assert_eq!(fs_inner.storage.len(), 1, "storage has a root");
        assert!(
            matches!(
                fs_inner.storage.get(ROOT_INODE),
                Some(Node::Directory {
                    inode: ROOT_INODE,
                    name,
                    children,
                    ..
                }) if name == "/" && children.is_empty(),
            ),
            "storage has a well-defined root",
        );
    }

    #[test]
    fn test_create_dir() {
        let fs = FileSystem::default();

        assert_eq!(
            fs.create_dir(path!("/")),
            Err(FsError::BaseNotDirectory),
            "creating a directory that has no parent",
        );

        assert_eq!(
            fs.create_dir(path!("/foo/..")),
            Err(FsError::InvalidInput),
            "invalid directory name",
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
                    Some(Node::Directory {
                        inode: ROOT_INODE,
                        name,
                        children,
                        ..
                    }) if name == "/" && children == &[1]
                ),
                "the root is updated and well-defined",
            );
            assert!(
                matches!(
                    fs_inner.storage.get(1),
                    Some(Node::Directory {
                        inode: 1,
                        name,
                        children,
                        ..
                    }) if name == "foo" && children.is_empty(),
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
                    Some(Node::Directory {
                        inode: ROOT_INODE,
                        name,
                        children,
                        ..
                    }) if name == "/" && children == &[1]
                ),
                "the root is updated again and well-defined",
            );
            assert!(
                matches!(
                    fs_inner.storage.get(1),
                    Some(Node::Directory {
                        inode: 1,
                        name,
                        children,
                        ..
                    }) if name == "foo" && children == &[2]
                ),
                "the new directory is updated and well-defined",
            );
            assert!(
                matches!(
                    fs_inner.storage.get(2),
                    Some(Node::Directory {
                        inode: 2,
                        name,
                        children,
                        ..
                    }) if name == "bar" && children.is_empty()
                ),
                "the new directory is well-defined",
            );
        }
    }

    #[test]
    fn test_remove_dir() {
        let fs = FileSystem::default();

        assert_eq!(
            fs.remove_dir(path!("/")),
            Err(FsError::BaseNotDirectory),
            "removing a directory that has no parent",
        );

        assert_eq!(
            fs.remove_dir(path!("/foo/..")),
            Err(FsError::InvalidInput),
            "invalid directory name",
        );

        assert_eq!(
            fs.remove_dir(path!("/foo")),
            Err(FsError::InvalidInput),
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

    #[test]
    fn test_rename() {
        let fs = FileSystem::default();

        assert_eq!(
            fs.rename(path!("/"), path!("/bar")),
            Err(FsError::BaseNotDirectory),
            "renaming a directory that has no parent",
        );
        assert_eq!(
            fs.rename(path!("/foo"), path!("/")),
            Err(FsError::BaseNotDirectory),
            "renaming to a directory that has no parent",
        );

        assert_eq!(
            fs.rename(path!("/foo/.."), path!("/bar")),
            Err(FsError::InvalidInput),
            "invalid directory name",
        );
        assert_eq!(
            fs.rename(path!("/foo"), path!("/bar/..")),
            Err(FsError::InvalidInput),
            "invalid directory name",
        );

        assert_eq!(fs.create_dir(path!("/foo")), Ok(()));
        assert_eq!(fs.create_dir(path!("/foo/qux")), Ok(()));

        assert_eq!(
            fs.rename(path!("/foo"), path!("/bar/baz")),
            Err(FsError::NotAFile),
            "renaming to a directory that has parent that doesn't exist",
        );

        assert_eq!(fs.create_dir(path!("/bar")), Ok(()));

        {
            let fs_inner = fs.inner.read().unwrap();

            assert_eq!(fs_inner.storage.len(), 4, "storage has all directories");
            assert!(
                matches!(
                    fs_inner.storage.get(ROOT_INODE),
                    Some(Node::Directory {
                        inode: ROOT_INODE,
                        name,
                        children,
                        ..
                    }) if name == "/" && children == &[1, 3]
                ),
                "`/` contains `foo` and `bar`",
            );
            assert!(
                matches!(
                    fs_inner.storage.get(1),
                    Some(Node::Directory {
                        inode: 1,
                        name,
                        children,
                        ..
                    }) if name == "foo" && children == &[2]
                ),
                "`foo` contains `qux`",
            );
            assert!(
                matches!(
                    fs_inner.storage.get(2),
                    Some(Node::Directory {
                        inode: 2,
                        name,
                        children,
                        ..
                    }) if name == "qux" && children.is_empty()
                ),
                "`qux` is empty",
            );
            assert!(
                matches!(
                    fs_inner.storage.get(3),
                    Some(Node::Directory {
                        inode: 3,
                        name,
                        children,
                        ..
                    }) if name == "bar" && children.is_empty()
                ),
                "`bar` is empty",
            );
        }

        assert_eq!(
            fs.rename(path!("/foo"), path!("/bar/baz")),
            Ok(()),
            "renaming a directory",
        );

        {
            let fs_inner = fs.inner.read().unwrap();

            assert_eq!(
                fs_inner.storage.len(),
                4,
                "storage has still all directories"
            );
            assert!(
                matches!(
                    fs_inner.storage.get(ROOT_INODE),
                    Some(Node::Directory {
                        inode: ROOT_INODE,
                        name,
                        children,
                        ..
                    }) if name == "/" && children == &[3]
                ),
                "`/` contains `bar`",
            );
            assert!(
                matches!(
                    fs_inner.storage.get(1),
                    Some(Node::Directory {
                        inode: 1,
                        name,
                        children,
                        ..
                    }) if name == "baz" && children == &[2]
                ),
                "`foo` has been renamed to `baz` and contains `qux`",
            );
            assert!(
                matches!(
                    fs_inner.storage.get(2),
                    Some(Node::Directory {
                        inode: 2,
                        name,
                        children,
                        ..
                    }) if name == "qux" && children.is_empty()
                ),
                "`qux` is empty",
            );
            assert!(
                matches!(
                    fs_inner.storage.get(3),
                    Some(Node::Directory {
                        inode: 3,
                        name,
                        children,
                        ..
                    }) if name == "bar" && children == &[1]
                ),
                "`bar` contains `baz` (ex `foo`)",
            );
        }
    }

    #[test]
    fn test_metadata() {
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

        assert_eq!(fs.rename(path!("/foo"), path!("/bar")), Ok(()));

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

    #[test]
    fn test_remove_file() {
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
                    Some(Node::Directory {
                        inode: ROOT_INODE,
                        name,
                        children,
                        ..
                    }) if name == "/" && children == &[1]
                ),
                "`/` contains `foo.txt`",
            );
            assert!(
                matches!(
                    fs_inner.storage.get(1),
                    Some(Node::File {
                        inode: 1,
                        name,
                        ..
                    }) if name == "foo.txt"
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
                    Some(Node::Directory {
                        inode: ROOT_INODE,
                        name,
                        children,
                        ..
                    }) if name == "/" && children == &[]
                ),
                "`/` is empty",
            );
        }

        assert_eq!(
            fs.remove_file(path!("/foo.txt")),
            Err(FsError::NotAFile),
            "removing a file that exists",
        );
    }
}

fn time() -> u64 {
    // SAFETY: It's very unlikely that the system returns a time that
    // is before `UNIX_EPOCH` :-).
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[derive(Debug, Clone)]
pub struct FileOpener {
    filesystem: FileSystem,
}

impl crate::FileOpener for FileOpener {
    fn open(&mut self, path: &Path, conf: &OpenOptionsConfig) -> Result<Box<dyn VirtualFile>> {
        let read = conf.read();
        let mut write = conf.write();
        let append = conf.append();
        let mut truncate = conf.truncate();
        let mut create = conf.create();
        let create_new = conf.create_new();

        // If `create_new` is used, `create` and `truncate ` are ignored.
        if create_new {
            create = false;
            truncate = false;
        }

        // To truncate a file, `write` must be used.
        if truncate && !write {
            return Err(FsError::PermissionDenied);
        }

        // `append` is semantically equivalent to `write` + `append`
        // but let's keep them exclusive.
        if append {
            write = false;
        }

        let (inode_of_parent, maybe_inode_of_file, name_of_file) = {
            // Read lock.
            let fs = self
                .filesystem
                .inner
                .try_read()
                .map_err(|_| FsError::Lock)?;

            // Check the path has a parent.
            let parent_of_path = path.parent().ok_or(FsError::BaseNotDirectory)?;

            // Check the file name.
            let name_of_file = path
                .file_name()
                .ok_or(FsError::InvalidInput)?
                .to_os_string();

            // Find the parent node.
            let inode_of_parent = fs.inode_of_parent(parent_of_path)?;

            // Find the inode of the file if it exists.
            let maybe_inode_of_file = fs
                .from_parent_get_position_and_inode_of_file(inode_of_parent, &name_of_file)?
                .map(|(_nth, inode)| inode);

            (inode_of_parent, maybe_inode_of_file, name_of_file)
        };

        let inode_of_file = match maybe_inode_of_file {
            // The file already exists, and a _new_ one _must_ be
            // created; it's not OK.
            Some(inode_of_file) if create_new => return Err(FsError::AlreadyExists),

            // The file already exists; it's OK.
            Some(inode_of_file) => {
                // Write lock.
                let mut fs = self
                    .filesystem
                    .inner
                    .try_write()
                    .map_err(|_| FsError::Lock)?;

                // Get the node of the file.
                let node = fs
                    .storage
                    .get_mut(inode_of_file)
                    .ok_or(FsError::UnknownError)?;

                match fs.storage.get_mut(inode_of_file) {
                    Some(Node::File { metadata, file, .. }) => {
                        // Update the accessed time.
                        metadata.accessed = time();

                        if truncate {
                            file.truncate();
                        }

                        if append {
                            file.seek(io::SeekFrom::End(0))?;
                        }
                    }

                    _ => return Err(FsError::NotAFile),
                }

                inode_of_file
            }

            // The file doesn't already exist; it's OK to create it if:
            // 1. `create_new` is used with `write` or `append`,
            // 2. `create` is used with `write` or `append`.
            None if (create_new || create) && (write || append) => {
                // Write lock.
                let mut fs = self
                    .filesystem
                    .inner
                    .try_write()
                    .map_err(|_| FsError::Lock)?;

                let file = File::new();

                // Creating the file in the storage.
                let inode_of_file = fs.storage.vacant_entry().key();
                let real_inode_of_file = fs.storage.insert(Node::File {
                    inode: inode_of_file,
                    name: name_of_file,
                    file,
                    metadata: {
                        let time = time();

                        Metadata {
                            ft: FileType {
                                file: true,
                                ..Default::default()
                            },
                            accessed: time,
                            created: time,
                            modified: time,
                            len: 0,
                        }
                    },
                });

                assert_eq!(
                    inode_of_file, real_inode_of_file,
                    "new file inode should have been correctly calculated",
                );

                // Adding the new directory to its parent.
                fs.add_child_to_node(inode_of_parent, inode_of_file)?;

                inode_of_file
            }

            None => return Err(FsError::PermissionDenied),
        };

        Ok(Box::new(FileHandle::new(
            inode_of_file,
            self.filesystem.clone(),
            read,
            write,
            append,
            truncate,
        )))
    }
}

#[cfg(test)]
mod test_file_opener {
    use super::*;
    use crate::FileSystem as FS;

    #[test]
    fn test_create_new_file() {
        let fs = FileSystem::default();

        assert!(
            matches!(
                fs.new_open_options()
                    .write(true)
                    .create_new(true)
                    .open(path!("/foo.txt")),
                Ok(_),
            ),
            "creating a new file",
        );

        {
            let fs_inner = fs.inner.read().unwrap();

            assert_eq!(fs_inner.storage.len(), 2, "storage has the new file");
            assert!(
                matches!(
                    fs_inner.storage.get(ROOT_INODE),
                    Some(Node::Directory {
                        inode: ROOT_INODE,
                        name,
                        children,
                        ..
                    }) if name == "/" && children == &[1]
                ),
                "`/` contains `foo.txt`",
            );
            assert!(
                matches!(
                    fs_inner.storage.get(1),
                    Some(Node::File {
                        inode: 1,
                        name,
                        ..
                    }) if name == "foo.txt"
                ),
                "`foo.txt` exists and is a file",
            );
        }

        assert!(
            matches!(
                fs.new_open_options()
                    .write(true)
                    .create_new(true)
                    .open(path!("/foo.txt")),
                Err(FsError::AlreadyExists)
            ),
            "creating a new file that already exist",
        );

        assert!(
            matches!(
                fs.new_open_options()
                    .write(true)
                    .create_new(true)
                    .open(path!("/foo/bar.txt")),
                Err(FsError::NotAFile),
            ),
            "creating a file in a directory that doesn't exist",
        );

        assert_eq!(fs.remove_file(path!("/foo.txt")), Ok(()), "removing a file");

        assert!(
            matches!(
                fs.new_open_options()
                    .write(false)
                    .create_new(true)
                    .open(path!("/foo.txt")),
                Err(FsError::PermissionDenied),
            ),
            "creating a file without the `write` option",
        );
    }

    #[test]
    fn test_truncate_a_read_only_file() {
        let fs = FileSystem::default();

        assert!(
            matches!(
                fs.new_open_options()
                    .write(false)
                    .truncate(true)
                    .open(path!("/foo.txt")),
                Err(FsError::PermissionDenied),
            ),
            "truncating a read-only file",
        );
    }

    #[test]
    fn test_opening_a_file_that_already_exists() {
        let fs = FileSystem::default();

        assert!(
            matches!(
                fs.new_open_options()
                    .write(true)
                    .create_new(true)
                    .open(path!("/foo.txt")),
                Ok(_),
            ),
            "creating a _new_ file",
        );

        assert!(
            matches!(
                fs.new_open_options()
                    .create_new(true)
                    .open(path!("/foo.txt")),
                Err(FsError::AlreadyExists),
            ),
            "creating a _new_ file that already exists",
        );

        assert!(
            matches!(
                fs.new_open_options().read(true).open(path!("/foo.txt")),
                Ok(_),
            ),
            "opening a file that already exists",
        );
    }
}

#[derive(Clone)]
struct FileHandle {
    inode: Inode,
    filesystem: FileSystem,
    read: bool,
    write: bool,
    append: bool,
    truncate: bool,
}

impl FileHandle {
    fn new(
        inode: Inode,
        filesystem: FileSystem,
        read: bool,
        write: bool,
        append: bool,
        truncate: bool,
    ) -> Self {
        Self {
            inode,
            filesystem,
            read,
            write,
            append,
            truncate,
        }
    }
}

impl VirtualFile for FileHandle {
    fn last_accessed(&self) -> u64 {
        let fs = match self.filesystem.inner.try_read() {
            Ok(fs) => fs,
            _ => return 0,
        };

        let node = match fs.storage.get(self.inode) {
            Some(node) => node,
            _ => return 0,
        };

        node.metadata().accessed
    }

    fn last_modified(&self) -> u64 {
        let fs = match self.filesystem.inner.try_read() {
            Ok(fs) => fs,
            _ => return 0,
        };

        let node = match fs.storage.get(self.inode) {
            Some(node) => node,
            _ => return 0,
        };

        node.metadata().modified
    }

    fn created_time(&self) -> u64 {
        let fs = match self.filesystem.inner.try_read() {
            Ok(fs) => fs,
            _ => return 0,
        };

        let node = match fs.storage.get(self.inode) {
            Some(node) => node,
            _ => return 0,
        };

        node.metadata().created
    }

    fn size(&self) -> u64 {
        let fs = match self.filesystem.inner.try_read() {
            Ok(fs) => fs,
            _ => return 0,
        };

        match fs.storage.get(self.inode) {
            Some(Node::File { file, .. }) => file.buffer.len().try_into().unwrap_or(0),
            _ => return 0,
        }
    }

    fn set_len(&mut self, new_size: u64) -> Result<()> {
        let mut fs = self
            .filesystem
            .inner
            .try_write()
            .map_err(|_| FsError::Lock)?;

        match fs.storage.get_mut(self.inode) {
            Some(Node::File { file, .. }) => file
                .buffer
                .resize(new_size.try_into().map_err(|_| FsError::UnknownError)?, 0),
            _ => return Err(FsError::NotAFile),
        }

        Ok(())
    }

    fn unlink(&mut self) -> Result<()> {
        let (inode_of_parent, position, inode_of_file) = {
            // Read lock.
            let fs = self
                .filesystem
                .inner
                .try_read()
                .map_err(|_| FsError::Lock)?;

            // The inode of the file.
            let inode_of_file = self.inode;

            // Find the position of the file in the parent, and the
            // inode of the parent.
            let (position, inode_of_parent) = fs
                .storage
                .iter()
                .find_map(|(inode_of_parent, node)| match node {
                    Node::Directory { children, .. } => {
                        children.iter().enumerate().find_map(|(nth, inode)| {
                            if inode == &inode_of_file {
                                Some((nth, inode_of_parent))
                            } else {
                                None
                            }
                        })
                    }

                    _ => None,
                })
                .ok_or(FsError::BaseNotDirectory)?;

            (inode_of_parent, position, inode_of_file)
        };

        {
            // Write lock.
            let mut fs = self
                .filesystem
                .inner
                .try_write()
                .map_err(|_| FsError::Lock)?;

            // Remove the file from the storage.
            fs.storage.remove(inode_of_file);

            // Remove the child from the parent directory.
            fs.remove_child_from_node(inode_of_parent, position)?;
        }

        Ok(())
    }

    fn bytes_available(&self) -> Result<usize> {
        let fs = self
            .filesystem
            .inner
            .try_read()
            .map_err(|_| FsError::Lock)?;

        match fs.storage.get(self.inode) {
            Some(Node::File { file, .. }) => Ok(file.buffer.len() - file.cursor),
            _ => Err(FsError::NotAFile),
        }
    }
}

#[cfg(test)]
mod test_file_handle_is_a_virtual_file {
    use super::*;
    use crate::FileSystem as FS;
    use std::thread::sleep;
    use std::time::Duration;

    #[test]
    fn test_last_accessed() {
        let fs = FileSystem::default();

        let file = fs
            .new_open_options()
            .write(true)
            .create_new(true)
            .open(path!("/foo.txt"))
            .expect("failed to create a new file");
        let last_accessed_time = file.last_accessed();

        assert!(last_accessed_time > 0, "last accessed time is not zero");

        sleep(Duration::from_secs(3));

        let file = fs
            .new_open_options()
            .read(true)
            .open(path!("/foo.txt"))
            .expect("failed to open a file");
        let next_last_accessed_time = file.last_accessed();

        assert!(
            next_last_accessed_time > last_accessed_time,
            "the last accessed time is updated"
        );
    }

    #[test]
    fn test_last_modified() {
        let fs = FileSystem::default();

        let file = fs
            .new_open_options()
            .write(true)
            .create_new(true)
            .open(path!("/foo.txt"))
            .expect("failed to create a new file");

        assert!(file.last_modified() > 0, "last modified time is not zero");
    }

    #[test]
    fn test_created_time() {
        let fs = FileSystem::default();

        let file = fs
            .new_open_options()
            .write(true)
            .create_new(true)
            .open(path!("/foo.txt"))
            .expect("failed to create a new file");
        let created_time = file.created_time();

        assert!(created_time > 0, "created time is not zero");

        let file = fs
            .new_open_options()
            .read(true)
            .open(path!("/foo.txt"))
            .expect("failed to create a new file");
        let next_created_time = file.created_time();

        assert_eq!(
            next_created_time, created_time,
            "created time stays constant"
        );
    }

    #[test]
    fn test_size() {
        let fs = FileSystem::default();

        let file = fs
            .new_open_options()
            .write(true)
            .create_new(true)
            .open(path!("/foo.txt"))
            .expect("failed to create a new file");

        assert_eq!(file.size(), 0, "new file is empty");
    }

    #[test]
    fn test_set_len() {
        let fs = FileSystem::default();

        let mut file = fs
            .new_open_options()
            .write(true)
            .create_new(true)
            .open(path!("/foo.txt"))
            .expect("failed to create a new file");
        file.set_len(7);

        assert_eq!(file.size(), 7, "file has a new length");
    }

    #[test]
    fn test_unlink() {
        let fs = FileSystem::default();

        let mut file = fs
            .new_open_options()
            .write(true)
            .create_new(true)
            .open(path!("/foo.txt"))
            .expect("failed to create a new file");

        {
            let fs_inner = fs.inner.read().unwrap();

            assert_eq!(fs_inner.storage.len(), 2, "storage has the new file");
            assert!(
                matches!(
                    fs_inner.storage.get(ROOT_INODE),
                    Some(Node::Directory {
                        inode: ROOT_INODE,
                        name,
                        children,
                        ..
                    }) if name == "/" && children == &[1]
                ),
                "`/` contains `foo.txt`",
            );
            assert!(
                matches!(
                    fs_inner.storage.get(1),
                    Some(Node::File {
                        inode: 1,
                        name,
                        ..
                    }) if name == "foo.txt"
                ),
                "`foo.txt` exists and is a file",
            );
        }

        assert_eq!(file.unlink(), Ok(()), "unlinking the file");

        {
            let fs_inner = fs.inner.read().unwrap();

            assert_eq!(
                fs_inner.storage.len(),
                1,
                "storage no longer has the new file"
            );
            assert!(
                matches!(
                    fs_inner.storage.get(ROOT_INODE),
                    Some(Node::Directory {
                        inode: ROOT_INODE,
                        name,
                        children,
                        ..
                    }) if name == "/" && children.is_empty()
                ),
                "`/` is empty",
            );
        }
    }

    #[test]
    fn test_bytes_available() {
        let fs = FileSystem::default();

        let mut file = fs
            .new_open_options()
            .write(true)
            .create_new(true)
            .open(path!("/foo.txt"))
            .expect("failed to create a new file");

        assert_eq!(file.bytes_available(), Ok(0), "zero bytes available");
        assert_eq!(file.set_len(7), Ok(()), "resizing the file");
        assert_eq!(file.bytes_available(), Ok(7), "seven bytes available");
    }
}

impl Read for FileHandle {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if !self.read {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "the file (inode `{}) doesn't have the `read` permission",
                    self.inode
                ),
            ));
        }

        let mut fs =
            self.filesystem.inner.try_write().map_err(|_| {
                io::Error::new(io::ErrorKind::Other, "failed to acquire a write lock")
            })?;

        let file = match fs.storage.get_mut(self.inode) {
            Some(Node::File { file, .. }) => file,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("inode `{}` doesn't match a file", self.inode),
                ))
            }
        };

        file.read(buf)
    }

    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
        if !self.read {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "the file (inode `{}) doesn't have the `read` permission",
                    self.inode
                ),
            ));
        }

        let mut fs =
            self.filesystem.inner.try_write().map_err(|_| {
                io::Error::new(io::ErrorKind::Other, "failed to acquire a write lock")
            })?;

        let file = match fs.storage.get_mut(self.inode) {
            Some(Node::File { file, .. }) => file,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("inode `{}` doesn't match a file", self.inode),
                ))
            }
        };

        file.read_to_end(buf)
    }

    fn read_to_string(&mut self, buf: &mut String) -> io::Result<usize> {
        // SAFETY: `String::as_mut_vec` cannot check that modifcations
        // of the `Vec` will produce a valid UTF-8 string. In our
        // case, we use `str::from_utf8` to ensure that the UTF-8
        // constraint still hold before returning.
        let mut bytes_buffer = unsafe { buf.as_mut_vec() };
        bytes_buffer.clear();
        let read = self.read_to_end(&mut bytes_buffer)?;

        if str::from_utf8(&bytes_buffer).is_err() {
            Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "buffer did not contain valid UTF-8",
            ))
        } else {
            Ok(read)
        }
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        if !self.read {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "the file (inode `{}) doesn't have the `read` permission",
                    self.inode
                ),
            ));
        }

        let mut fs =
            self.filesystem.inner.try_write().map_err(|_| {
                io::Error::new(io::ErrorKind::Other, "failed to acquire a write lock")
            })?;

        let file = match fs.storage.get_mut(self.inode) {
            Some(Node::File { file, .. }) => file,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("inode `{}` doesn't match a file", self.inode),
                ))
            }
        };

        file.read_exact(buf)
    }
}

impl Seek for FileHandle {
    fn seek(&mut self, position: io::SeekFrom) -> io::Result<u64> {
        let mut fs =
            self.filesystem.inner.try_write().map_err(|_| {
                io::Error::new(io::ErrorKind::Other, "failed to acquire a write lock")
            })?;

        let file = match fs.storage.get_mut(self.inode) {
            Some(Node::File { file, .. }) => file,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("inode `{}` doesn't match a file", self.inode),
                ))
            }
        };

        file.seek(position)
    }
}

impl Write for FileHandle {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if !self.write {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "the file (inode `{}) doesn't have the `write` permission",
                    self.inode
                ),
            ));
        }

        let mut fs =
            self.filesystem.inner.try_write().map_err(|_| {
                io::Error::new(io::ErrorKind::Other, "failed to acquire a write lock")
            })?;

        let file = match fs.storage.get_mut(self.inode) {
            Some(Node::File { file, .. }) => file,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("inode `{}` doesn't match a file", self.inode),
                ))
            }
        };

        file.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        self.write(buf);

        Ok(())
    }
}

#[cfg(test)]
mod test_file_handle_can_read_write_and_seek {
    use super::*;
    use crate::FileSystem as FS;

    #[test]
    fn test_writing_at_various_positions() {
        let fs = FileSystem::default();

        let mut file = fs
            .new_open_options()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path!("/foo.txt"))
            .expect("failed to create a new file");

        assert!(
            matches!(file.write(b"foo"), Ok(3)),
            "writing `foo` at the end of the file"
        );
        assert_eq!(file.size(), 3, "checking the size of the file");

        assert!(
            matches!(file.write(b"bar"), Ok(3)),
            "writing `bar` at the end of the file"
        );
        assert_eq!(file.size(), 6, "checking the size of the file");

        assert!(
            matches!(file.seek(io::SeekFrom::Start(0)), Ok(0)),
            "seeking to 0"
        );

        assert!(
            matches!(file.write(b"baz"), Ok(3)),
            "writing `baz` at the beginning of the file"
        );
        assert_eq!(file.size(), 9, "checking the size of the file");

        assert!(
            matches!(file.write(b"qux"), Ok(3)),
            "writing `qux` in the middle of the file"
        );
        assert_eq!(file.size(), 12, "checking the size of the file");

        assert!(
            matches!(file.seek(io::SeekFrom::Start(0)), Ok(0)),
            "seeking to 0"
        );

        let mut string = String::new();
        assert!(
            matches!(file.read_to_string(&mut string), Ok(12)),
            "reading `bazquxfoobar`"
        );
        assert_eq!(string, "bazquxfoobar");

        assert!(
            matches!(file.seek(io::SeekFrom::Current(-6)), Ok(6)),
            "seeking to 6"
        );

        let mut string = String::new();
        assert!(
            matches!(file.read_to_string(&mut string), Ok(6)),
            "reading `foobar`"
        );
        assert_eq!(string, "foobar");

        assert!(
            matches!(file.seek(io::SeekFrom::End(0)), Ok(12)),
            "seeking to 12"
        );

        let mut string = String::new();
        assert!(
            matches!(file.read_to_string(&mut string), Ok(0)),
            "reading ``"
        );
        assert_eq!(string, "");
    }

    #[test]
    fn test_reading() {
        let fs = FileSystem::default();

        let mut file = fs
            .new_open_options()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path!("/foo.txt"))
            .expect("failed to create a new file");

        assert!(
            matches!(file.write(b"foobarbazqux"), Ok(12)),
            "writing `foobarbazqux`"
        );

        assert!(
            matches!(file.seek(io::SeekFrom::Start(0)), Ok(0)),
            "seeking to 0"
        );

        let mut buffer = [0; 6];
        assert!(
            matches!(file.read(&mut buffer[..]), Ok(6)),
            "reading 6 bytes"
        );
        assert_eq!(buffer, b"foobar"[..], "checking the 6 bytes");

        assert!(
            matches!(file.seek(io::SeekFrom::Start(0)), Ok(0)),
            "seeking to 0"
        );

        let mut buffer = [0; 16];
        assert!(
            matches!(file.read(&mut buffer[..]), Ok(12)),
            "reading more bytes than available",
        );
        assert_eq!(buffer[..12], b"foobarbazqux"[..], "checking the 12 bytes");
    }

    #[test]
    fn test_reading_to_the_end() {
        let fs = FileSystem::default();

        let mut file = fs
            .new_open_options()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path!("/foo.txt"))
            .expect("failed to create a new file");

        assert!(
            matches!(file.write(b"foobarbazqux"), Ok(12)),
            "writing `foobarbazqux`"
        );

        assert!(
            matches!(file.seek(io::SeekFrom::Start(0)), Ok(0)),
            "seeking to 0"
        );

        let mut buffer = Vec::new();
        assert!(
            matches!(file.read_to_end(&mut buffer), Ok(12)),
            "reading all bytes"
        );
        assert_eq!(buffer, b"foobarbazqux"[..], "checking all the bytes");
    }

    #[test]
    fn test_reading_to_string() {
        let fs = FileSystem::default();

        let mut file = fs
            .new_open_options()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path!("/foo.txt"))
            .expect("failed to create a new file");

        assert!(
            matches!(file.write(b"foobarbazqux"), Ok(12)),
            "writing `foobarbazqux`"
        );

        assert!(
            matches!(file.seek(io::SeekFrom::Start(6)), Ok(6)),
            "seeking to 0"
        );

        let mut string = String::new();
        assert!(
            matches!(file.read_to_string(&mut string), Ok(6)),
            "reading a string"
        );
        assert_eq!(string, "bazqux", "checking the string");
    }

    #[test]
    fn test_reading_exact_buffer() {
        let fs = FileSystem::default();

        let mut file = fs
            .new_open_options()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path!("/foo.txt"))
            .expect("failed to create a new file");

        assert!(
            matches!(file.write(b"foobarbazqux"), Ok(12)),
            "writing `foobarbazqux`"
        );

        assert!(
            matches!(file.seek(io::SeekFrom::Start(6)), Ok(6)),
            "seeking to 0"
        );

        let mut buffer = [0; 16];
        assert!(
            matches!(file.read_exact(&mut buffer), Err(_)),
            "failing to read an exact buffer"
        );

        assert!(
            matches!(file.seek(io::SeekFrom::End(-5)), Ok(7)),
            "seeking to 7"
        );

        let mut buffer = [0; 3];
        assert!(
            matches!(file.read_exact(&mut buffer), Ok(())),
            "failing to read an exact buffer"
        );
    }
}

impl fmt::Debug for FileHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FileHandle")
            .field("inode", &self.inode)
            .finish()
    }
}

#[derive(Debug)]
struct File {
    buffer: Vec<u8>,
    cursor: usize,
}

impl File {
    fn new() -> Self {
        Self {
            buffer: Vec::new(),
            cursor: 0,
        }
    }

    fn truncate(&mut self) {
        self.buffer.clear();
        self.cursor = 0;
    }
}

impl Read for File {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let max_to_read = cmp::min(self.buffer.len() - self.cursor, buf.len());
        let data_to_copy = &self.buffer[self.cursor..][..max_to_read];

        // SAFETY: `buf[..max_to_read]` and `data_to_copy` have the same size, due to
        // how `max_to_read` is computed.
        buf[..max_to_read].copy_from_slice(data_to_copy);

        self.cursor += max_to_read;

        Ok(max_to_read)
    }

    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
        let data_to_copy = &self.buffer[self.cursor..];
        let max_to_read = data_to_copy.len();

        // `buf` is too small to contain the data. Let's resize it.
        if max_to_read > buf.len() {
            // Let's resize the capacity if needed.
            if max_to_read > buf.capacity() {
                buf.reserve_exact(max_to_read - buf.capacity());
            }

            // SAFETY: The space is reserved, and it's going to be
            // filled with `copy_from_slice` below.
            unsafe { buf.set_len(max_to_read) }
        }

        // SAFETY: `buf` and `data_to_copy` have the same size, see
        // above.
        buf.copy_from_slice(data_to_copy);

        self.cursor += max_to_read;

        Ok(max_to_read)
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        if buf.len() > (self.buffer.len() - self.cursor) {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "not enough data available in file",
            ));
        }

        let max_to_read = cmp::min(buf.len(), self.buffer.len() - self.cursor);
        let data_to_copy = &self.buffer[self.cursor..][..max_to_read];

        // SAFETY: `buf` and `data_to_copy` have the same size.
        buf.copy_from_slice(data_to_copy);

        self.cursor += data_to_copy.len();

        Ok(())
    }
}

impl Seek for File {
    fn seek(&mut self, position: io::SeekFrom) -> io::Result<u64> {
        let to_err = |_| io::ErrorKind::InvalidInput;

        // Calculate the next cursor.
        let next_cursor: i64 = match position {
            // Calculate from the beginning, so `0 + offset`.
            io::SeekFrom::Start(offset) => offset.try_into().map_err(to_err)?,

            // Calculate from the end, so `buffer.len() + offset`.
            io::SeekFrom::End(offset) => {
                TryInto::<i64>::try_into(self.buffer.len()).map_err(to_err)? + offset
            }

            // Calculate from the current cursor, so `cursor + offset`.
            io::SeekFrom::Current(offset) => {
                TryInto::<i64>::try_into(self.cursor).map_err(to_err)? + offset
            }
        };

        // It's an error to seek before byte 0.
        if next_cursor < 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "seeking before the byte 0",
            ));
        }

        // In this implementation, it's an error to seek beyond the
        // end of the buffer.
        self.cursor = cmp::min(self.buffer.len(), next_cursor.try_into().map_err(to_err)?);

        Ok(self.cursor.try_into().map_err(to_err)?)
    }
}

impl Write for File {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self.cursor {
            // The cursor is at the end of the buffer: happy path!
            position if position == self.buffer.len() => {
                self.buffer.extend_from_slice(buf);
                self.cursor += buf.len();
            }

            // The cursor is at the beginning of the buffer (and the
            // buffer is not empty, otherwise it would have been
            // caught by the previous arm): almost a happy path!
            0 => {
                let mut new_buffer = Vec::with_capacity(self.buffer.len() + buf.len());
                new_buffer.extend_from_slice(buf);
                new_buffer.append(&mut self.buffer);

                self.buffer = new_buffer;
                self.cursor += buf.len();
            }

            // The cursor is somewhere in the buffer: not the happy path.
            position => {
                self.buffer.reserve_exact(buf.len());

                let mut remainder = self.buffer.split_off(position);
                self.buffer.extend_from_slice(buf);
                self.buffer.append(&mut remainder);

                self.cursor += buf.len();
            }
        }

        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/*
/// A wrapper type around Stdout that implements `VirtualFile`
#[derive(Debug, Default)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct Stdout {
    pub buf: Vec<u8>,
}

impl Read for Stdout {
    fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "can not read from stdout",
        ))
    }

    fn read_to_end(&mut self, _buf: &mut Vec<u8>) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "can not read from stdout",
        ))
    }

    fn read_to_string(&mut self, _buf: &mut String) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "can not read from stdout",
        ))
    }

    fn read_exact(&mut self, _buf: &mut [u8]) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "can not read from stdout",
        ))
    }
}

impl Seek for Stdout {
    fn seek(&mut self, _pos: io::SeekFrom) -> io::Result<u64> {
        Err(io::Error::new(io::ErrorKind::Other, "can not seek stdout"))
    }
}

impl Write for Stdout {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        // io::stdout().write(buf)
        unimplemented!();
    }

    fn flush(&mut self) -> io::Result<()> {
        // io::stdout().flush()
        // unimplemented!();
        Ok(())
    }

    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        // io::stdout().write_all(buf)
        self.buf.extend_from_slice(&buf);
        Ok(())
    }

    fn write_fmt(&mut self, _fmt: ::std::fmt::Arguments) -> io::Result<()> {
        // io::stdout().write_fmt(fmt)
        unimplemented!();
    }
}

#[cfg_attr(feature = "enable-serde", typetag::serde)]
impl VirtualFile for Stdout {
    fn last_accessed(&self) -> u64 {
        0
    }

    fn last_modified(&self) -> u64 {
        0
    }

    fn created_time(&self) -> u64 {
        0
    }

    fn size(&self) -> u64 {
        0
    }

    fn set_len(&mut self, _new_size: u64) -> Result<()> {
        debug!("Calling VirtualFile::set_len on stdout; this is probably a bug");
        Err(FsError::PermissionDenied)
    }

    fn unlink(&mut self) -> Result<()> {
        Ok(())
    }

    fn bytes_available(&self) -> Result<usize> {
        // unwrap is safe because of get_raw_fd implementation
        unimplemented!();
    }
}

/// A wrapper type around Stderr that implements `VirtualFile` and
/// `Serialize` + `Deserialize`.
#[derive(Debug, Default)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct Stderr {
    pub buf: Vec<u8>,
}

impl Read for Stderr {
    fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "can not read from stderr",
        ))
    }

    fn read_to_end(&mut self, _buf: &mut Vec<u8>) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "can not read from stderr",
        ))
    }

    fn read_to_string(&mut self, _buf: &mut String) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "can not read from stderr",
        ))
    }

    fn read_exact(&mut self, _buf: &mut [u8]) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "can not read from stderr",
        ))
    }
}

impl Seek for Stderr {
    fn seek(&mut self, _pos: io::SeekFrom) -> io::Result<u64> {
        Err(io::Error::new(io::ErrorKind::Other, "can not seek stderr"))
    }
}

impl Write for Stderr {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        // io::stderr().write(buf)
        unimplemented!();
    }

    fn flush(&mut self) -> io::Result<()> {
        // io::stderr().flush()
        // unimplemented!();
        Ok(())
    }

    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        self.buf.extend_from_slice(&buf);
        Ok(())
        // io::stderr().write_all(buf)
        // unimplemented!();
    }

    fn write_fmt(&mut self, _fmt: ::std::fmt::Arguments) -> io::Result<()> {
        // io::stderr().write_fmt(fmt)
        unimplemented!();
    }
}

#[cfg_attr(feature = "enable-serde", typetag::serde)]
impl VirtualFile for Stderr {
    fn last_accessed(&self) -> u64 {
        0
    }

    fn last_modified(&self) -> u64 {
        0
    }

    fn created_time(&self) -> u64 {
        0
    }

    fn size(&self) -> u64 {
        0
    }

    fn set_len(&mut self, _new_size: u64) -> Result<()> {
        debug!("Calling VirtualFile::set_len on stderr; this is probably a bug");
        Err(FsError::PermissionDenied)
    }

    fn unlink(&mut self) -> Result<()> {
        Ok(())
    }

    fn bytes_available(&self) -> Result<usize> {
        unimplemented!();
    }
}

/// A wrapper type around Stdin that implements `VirtualFile` and
/// `Serialize` + `Deserialize`.
#[derive(Debug, Default)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct Stdin {
    pub buf: Vec<u8>,
}

impl Read for Stdin {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let len = std::cmp::min(buf.len(), self.buf.len());
        for (i, val) in self.buf.drain(..len).enumerate() {
            buf[i] = val;
        }
        Ok(len)
        // unimplemented!();
    }

    fn read_to_end(&mut self, _buf: &mut Vec<u8>) -> io::Result<usize> {
        // io::stdin().read_to_end(buf)
        unimplemented!();
    }

    fn read_to_string(&mut self, _buf: &mut String) -> io::Result<usize> {
        // io::stdin().read_to_string(buf)
        unimplemented!();
    }

    fn read_exact(&mut self, _buf: &mut [u8]) -> io::Result<()> {
        // io::stdin().read_exact(buf)
        unimplemented!();
    }
}

impl Seek for Stdin {
    fn seek(&mut self, _pos: io::SeekFrom) -> io::Result<u64> {
        Err(io::Error::new(io::ErrorKind::Other, "can not seek stdin"))
    }
}

impl Write for Stdin {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "can not write to stdin",
        ))
    }

    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "can not write to stdin",
        ))
    }

    fn write_all(&mut self, _buf: &[u8]) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "can not write to stdin",
        ))
    }

    fn write_fmt(&mut self, _fmt: ::std::fmt::Arguments) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "can not write to stdin",
        ))
    }
}

#[cfg_attr(feature = "enable-serde", typetag::serde)]
impl VirtualFile for Stdin {
    fn last_accessed(&self) -> u64 {
        0
    }

    fn last_modified(&self) -> u64 {
        0
    }

    fn created_time(&self) -> u64 {
        0
    }

    fn size(&self) -> u64 {
        0
    }

    fn set_len(&mut self, _new_size: u64) -> Result<()> {
        debug!("Calling VirtualFile::set_len on stdin; this is probably a bug");
        Err(FsError::PermissionDenied)
    }

    fn unlink(&mut self) -> Result<()> {
        Ok(())
    }

    fn bytes_available(&self) -> Result<usize> {
        unimplemented!();
    }
}
*/
