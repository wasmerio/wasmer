use super::{
    ArcDirectoryNode, CustomFileNode, DirectoryNode, FileHandle, FileNode, FileSystem, Inode, Node,
    ReadOnlyFileNode,
};
use crate::FileSystem as _;
use crate::{Descriptor, DescriptorType, DirectoryEntry, FsError, ReaddirIterator, Result};
use std::ffi::OsString;
use std::path::{Component, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct Directory {
    inode: Inode,
    unique_id: usize,
    fs: FileSystem,
}

impl Directory {
    pub fn new(inode: Inode, fs: FileSystem) -> Self {
        Self {
            inode,
            fs,
            unique_id: crate::generate_next_unique_id(),
        }
    }
}

impl crate::Directory for Directory {
    fn unique_id(&self) -> usize {
        self.unique_id
    }

    fn get_child(&self, name: OsString) -> Result<Descriptor> {
        let guard = self.fs.inner.read().unwrap();
        let node = guard.get_node_directory(self.inode).unwrap();
        let found_node = node
            .children
            .iter()
            .find_map(|inode| {
                guard.storage.get(*inode).and_then(|node| {
                    if node.name() == name {
                        Some(node)
                    } else {
                        None
                    }
                })
            })
            .ok_or(FsError::EntryNotFound)?;
        match found_node {
            Node::Directory(DirectoryNode { inode, .. }) => {
                let directory = Directory::new(*inode, self.fs.clone());
                Ok(Descriptor::Directory(Arc::new(directory)))
            }
            Node::File(FileNode { inode, .. })
            | Node::ReadOnlyFile(ReadOnlyFileNode { inode, .. })
            | Node::CustomFile(CustomFileNode { inode, .. }) => {
                let file = FileHandle::new(*inode, self.fs.clone(), false, false, false, 0);
                Ok(Descriptor::File(Arc::new(file)))
            }
            Node::ArcDirectory(ArcDirectoryNode { fs, .. }) => {
                Ok(Descriptor::Directory(Arc::new(fs.as_dir())))
            }
        }
    }

    fn iter(&self) -> ReaddirIterator {
        let guard = self.fs.inner.read().unwrap();
        let node = guard.get_node_directory(self.inode).unwrap();
        let fs = self.fs.clone();
        ReaddirIterator(Mutex::new(Box::new(node.children.clone().into_iter().map(
            move |child_inode| {
                let guard = fs.inner.read().unwrap();
                let node = guard.storage.get(child_inode).unwrap();
                Ok(DirectoryEntry {
                    type_: match node {
                        Node::Directory(_) => DescriptorType::Directory,
                        Node::File(_) => DescriptorType::File,
                        Node::ReadOnlyFile(_) => DescriptorType::File,
                        Node::CustomFile(_) => DescriptorType::File,
                        Node::ArcDirectory(_) => DescriptorType::Directory,
                    },
                    name: node.name().to_owned(),
                })
            },
        ))))
    }
    fn absolute_path(&self) -> PathBuf {
        let guard = self.fs.inner.read().unwrap();
        let node = guard.get_node(self.inode).unwrap();
        guard.absolute_path(node)
    }

    fn walk_to<'a>(&self, to: PathBuf) -> Result<Arc<dyn crate::Directory + Send + Sync>> {
        if to == PathBuf::from(".") || to == PathBuf::from("") {
            return Ok(Arc::new(self.clone()));
        }
        let guard = self.fs.inner.read().map_err(|_| FsError::Lock)?;
        let mut node = guard.storage.get(self.inode).unwrap();

        let mut to = to.components();
        while let Some(component) = to.next() {
            node = match node {
                Node::Directory(DirectoryNode { children, .. }) => match component {
                    Component::CurDir => node,
                    Component::ParentDir => {
                        let parent_inode = node.parent_inode();
                        if parent_inode == super::ROOT_INODE {
                            let remaining_components: PathBuf = to.collect();
                            if let Some(parent) = &guard.parent {
                                return parent.walk_to(remaining_components);
                            } else {
                                return Err(FsError::BaseNotDirectory);
                            }
                        } else {
                            guard.storage.get(parent_inode).unwrap()
                        }
                    }
                    Component::Normal(name) => children
                        .iter()
                        .find_map(|inode| {
                            guard.storage.get(*inode).and_then(|node| {
                                if node.name() == name {
                                    Some(node)
                                } else {
                                    None
                                }
                            })
                        })
                        .ok_or(FsError::EntryNotFound)?,
                    _ => return Err(FsError::InvalidData),
                },
                // Node::File(FileNode {inode,..}) | Node::ReadOnlyFile(ReadOnlyFileNode { inode, ..}) => {
                //     // We are trying to get a path from a file
                //     if to.next().is_some() {
                //         return Err(FsError::BaseNotDirectory);
                //     }
                //     drop(guard);
                //     let file = FileHandle::new(
                //         *inode,
                //         self.fs.clone(),
                //         false,
                //         false,
                //         false,
                //         0,
                //     );
                //     return Ok(Descriptor::File(Box::new(file)));
                // },
                // Node::CustomFile(CustomFileNode { file, ..}) => {
                //     drop(guard);
                //     // We are trying to get a path from a file
                //     if to.next().is_some() {
                //         return Err(FsError::BaseNotDirectory);
                //     }
                //     return Ok(Descriptor::File(Box::new(file)));
                // },
                Node::ArcDirectory(ArcDirectoryNode { fs, .. }) => {
                    let remaining_components: PathBuf = to.collect();
                    return fs.as_dir().walk_to(remaining_components);
                }
                _ => return Err(FsError::BaseNotDirectory),
            };
        }
        match node {
            Node::Directory(DirectoryNode { inode, .. }) => {
                let directory = Directory::new(*inode, self.fs.clone());
                Ok(Arc::new(directory))
            }
            _ => Err(FsError::BaseNotDirectory),
        }
    }

    fn parent(&self) -> Option<Arc<dyn crate::Directory + Send + Sync>> {
        unimplemented!();
        // let parent_inode = {
        //     let guard = self.fs.inner.read().ok()?;
        //     guard.get_node_directory(self.inode).ok()?.parent_inode
        // };
        // if parent_inode == super::ROOT_INODE {
        //     return self.fs.parent.clone();
        // }
        // Some(Arc::new(Directory::new(parent_inode, self.fs)))
    }

    // fn read_dir(&self, path: &Path) -> Result<ReadDir> {
    //     self.fs.read_dir(path)
    // }

    // fn create_dir(&self, path: &Path) -> Result<()> {
    //     self.fs.create_dir(path)
    // }

    // fn remove_dir(&self, path: &Path) -> Result<()> {
    //     self.fs.remove_dir(path)
    // }

    // fn rename<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, Result<()>> {
    //     self.fs.rename(from, to)
    // }

    // fn metadata(&self, path: &Path) -> Result<Metadata> {
    //     self.fs.metadata(path)
    // }

    // fn remove_file(&self, path: &Path) -> Result<()> {
    //     self.fs.remove_file(path)
    // }

    // fn new_open_options(&self) -> OpenOptions {
    //     self.fs.new_open_options()
    // }
}

#[cfg(test)]
mod tests {
    use super::FileSystem;
    use crate::overlay_fs::OverlayFileSystem;
    use crate::WebcVolumeFileSystem;
    use crate::{DescriptorType, DirectoryEntry, FileSystem as _, FsError};
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};

    const PYTHON_WEBC: &[u8] = include_bytes!("../../../c-api/examples/assets/python-0.1.0.wasmer");

    #[tokio::test]
    async fn test_create_dir_dot() -> anyhow::Result<()> {
        let fs = FileSystem::default();
        // fs.create_dir(".").unwrap();
        assert_eq!(
            fs.create_dir(&PathBuf::from("/a")),
            Ok(()),
            "creating the root which already exists",
        );

        let dir = fs.as_dir();
        let iter_items = dir.iter().into_iter().collect::<Vec<_>>();
        assert_eq!(iter_items.len(), 1);
        assert_eq!(
            iter_items[0].as_ref().unwrap(),
            &DirectoryEntry {
                type_: DescriptorType::Directory,
                name: OsString::from("a"),
            }
        );
        let dir_a = dir.walk_to(PathBuf::from("a"))?;

        assert_eq!(dir_a.absolute_path(), PathBuf::from("/a"));
        assert_eq!(fs.create_dir(&PathBuf::from("/a/b")), Ok(()),);

        assert_eq!(fs.create_dir(&PathBuf::from("/b")), Ok(()),);

        let dir_b = dir.walk_to(PathBuf::from("b"))?;

        let dir_a_b = dir_a.walk_to(PathBuf::from("b"))?;

        fs.rename(&PathBuf::from("/a"), &PathBuf::from("/b/a"))
            .await?;

        assert_eq!(dir_a.absolute_path(), PathBuf::from("/b/a"));
        assert_eq!(dir_a_b.absolute_path(), PathBuf::from("/b/a/b"));

        Ok(())
    }

    #[tokio::test]
    async fn test_create_dir_overlay() -> anyhow::Result<()> {
        let fs = FileSystem::default();
        assert_eq!(
            fs.create_dir(&PathBuf::from("/a")),
            Ok(()),
            "creating the root which already exists",
        );

        assert_eq!(fs.create_dir(&PathBuf::from("/a/b")), Ok(()),);
        assert_eq!(fs.create_dir(&PathBuf::from("/b")), Ok(()),);

        let fs2 = FileSystem::default();
        assert_eq!(fs2.create_dir(&PathBuf::from("/d")), Ok(()),);
        assert_eq!(fs2.create_dir(&PathBuf::from("/e")), Ok(()),);

        let overlay = OverlayFileSystem::new(fs.clone(), [fs2]);

        // let dir = dbg!(overlay.primary.as_dir().walk_to(PathBuf::from(".")));

        let dir = overlay.as_dir().walk_to(PathBuf::from("."))?;
        // dbg!(dir.iter().into_iter().collect::<Vec<_>>());

        Ok(())
    }
}
