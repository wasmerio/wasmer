use super::{FileSystem, FileHandle, Inode, FileNode, DirectoryNode, ArcDirectoryNode, ReadOnlyFileNode, CustomFileNode, Node};
use crate::FileSystem as _;
use crate::{DirEntry, FileType, FsError, Metadata, OpenOptions, ReadDir, Result};
use futures::future::BoxFuture;
use std::path::{Path, PathBuf, Components, Component};
use std::sync::Arc;
use std::ffi::OsString;

#[derive(Debug, Clone)]
pub struct Directory {
    inode: Inode,
    unique_id: usize,
    fs: FileSystem,
}

pub enum DescriptorType {
    File,
    Directory,
}

pub struct DirectoryEntry {
    pub(crate) type_: DescriptorType,
    pub(crate) name: OsString,
}

// pub enum Descriptor {
//     File(Box<dyn crate::VirtualFile>),
//     Directory(Box<dyn crate::Directory>),
// }

pub struct ReaddirIterator(
    std::sync::Mutex<Box<dyn Iterator<Item = Result<DirectoryEntry>> + Send + 'static>>,
);

impl ReaddirIterator {
    pub(crate) fn new(
        i: impl Iterator<Item = Result<DirectoryEntry>> + Send + 'static,
    ) -> Self {
        ReaddirIterator(std::sync::Mutex::new(Box::new(i)))
    }
    pub(crate) fn next(&self) -> Result<Option<DirectoryEntry>> {
        self.0.lock().unwrap().next().transpose()
    }
}

impl IntoIterator for ReaddirIterator {
    type Item = Result<DirectoryEntry>;
    type IntoIter = Box<dyn Iterator<Item = Self::Item> + Send>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_inner().unwrap()
    }
}

impl Directory {
    pub fn new(inode: Inode, fs: FileSystem) -> Self {
        Self {
            inode,
            fs,
            unique_id: crate::generate_next_unique_id(),
        }
    }

    fn children(self) -> ReaddirIterator {
        unimplemented!();
    }

    // fn get_child(self, name: OsString) -> Option<Descriptor> {
    //     unimplemented!();
    // }

    fn remove_child(&self, name: OsString) -> Result<()> {
        // let child = self.clone().get_child(name).ok_or(FsError::EntryNotFound)?;
        unimplemented!();
    }
}

impl crate::Directory for Directory {
    fn unique_id(&self) -> usize {
        self.unique_id
    }

    fn walk_to<'a>(&self, to: PathBuf) -> Result<Box<dyn crate::Directory + Send>> {
        let guard = self.fs.inner.read().map_err(|_| FsError::Lock)?;
        let mut node = guard.storage.get(self.inode).unwrap();

        let to = to.components();
        while let Some(component) = to.next() {
            node = match node {
                Node::Directory(DirectoryNode { children, .. }) => {
                    match component {
                        Component::CurDir => node,
                        Component::ParentDir => {
                            let parent_inode = node.parent_inode();
                            if parent_inode == super::ROOT_INODE {
                                drop(guard);
                                let remaining_components: PathBuf = to.collect();
                                if let Some(parent) = guard.parent {
                                    return parent.as_dir().walk_to(remaining_components)?;
                                }
                                else {
                                    return Err(FsError::BaseNotDirectory);
                                }
                            }
                            else {
                                guard.storage.get(parent_inode).unwrap()
                            }
                        },
                        Component::Normal(name) => {
                            children
                                .iter()
                                .find_map(|inode| guard.storage.get(*inode).and_then(|node| {
                                    if node.name() == name {
                                        Some(node)
                                    }
                                    else {
                                        None
                                    }
                                })).ok_or(Err(FsError::EntryNotFound))?
                        }
                    }
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
                    drop(guard);
                    let remaining_components: PathBuf = to.collect();
                    return fs.as_dir().walk_to(remaining_components);
                }
                _ => return Err(FsError::BaseNotDirectory),
            };
        }
        match node {
            Node::Directory(DirectoryNode { inode, .. }) => {
                let directory = Directory::new(*inode, self.fs.clone());
                return Ok(Box::new(directory));
            }
            _ => return Err(FsError::BaseNotDirectory),
        }
    }

    fn parent(self) -> Option<Box<dyn crate::Directory + Send>> {
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
    use crate::{FsError, FileSystem as _};
    use std::path::{Path, PathBuf};

#[tokio::test]
async fn test_create_dir_dot() -> anyhow::Result<()> {
    let fs = FileSystem::default();
    // fs.create_dir(".").unwrap();
    assert_eq!(
        fs.create_dir(&PathBuf::from("/base_dir")),
        Ok(()),
        "creating the root which already exists",
    );

    let dir = fs.as_dir();
    assert!(dir.parent().is_none());
    Ok(())

}
}