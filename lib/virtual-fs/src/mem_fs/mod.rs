mod dir;
mod file;
mod file_opener;
mod filesystem;
mod stdio;

use self::dir::Directory;
use self::file::{File, FileHandle, ReadOnlyFile};
pub use self::filesystem::FileSystem;
pub use self::stdio::{Stderr, Stdin, Stdout};

use crate::Metadata;
use std::{
    ffi::{OsStr, OsString},
    path::PathBuf,
    sync::{Arc, Mutex},
};

type Inode = usize;
const ROOT_INODE: Inode = 0;

#[derive(Debug)]
struct FileNode {
    inode: Inode,
    parent_inode: Inode,
    name: OsString,
    file: File,
    metadata: Metadata,
}

#[derive(Debug)]
struct ReadOnlyFileNode {
    inode: Inode,
    parent_inode: Inode,
    name: OsString,
    file: ReadOnlyFile,
    metadata: Metadata,
}

#[derive(Debug)]
struct CustomFileNode {
    inode: Inode,
    parent_inode: Inode,
    name: OsString,
    file: Mutex<Box<dyn crate::VirtualFile + Send + Sync>>,
    metadata: Metadata,
}

#[derive(Debug)]
struct DirectoryNode {
    inode: Inode,
    parent_inode: Inode,
    name: OsString,
    children: Vec<Inode>,
    metadata: Metadata,
}

#[derive(Debug)]
struct ArcDirectoryNode {
    inode: Inode,
    parent_inode: Inode,
    name: OsString,
    fs: Arc<dyn crate::FileSystem + Send + Sync>,
    path: PathBuf,
    metadata: Metadata,
}

#[derive(Debug)]
enum Node {
    File(FileNode),
    ReadOnlyFile(ReadOnlyFileNode),
    CustomFile(CustomFileNode),
    Directory(DirectoryNode),
    ArcDirectory(ArcDirectoryNode),
}

impl Node {
    fn inode(&self) -> Inode {
        *match self {
            Self::File(FileNode { inode, .. }) => inode,
            Self::ReadOnlyFile(ReadOnlyFileNode { inode, .. }) => inode,
            Self::CustomFile(CustomFileNode { inode, .. }) => inode,
            Self::Directory(DirectoryNode { inode, .. }) => inode,
            Self::ArcDirectory(ArcDirectoryNode { inode, .. }) => inode,
        }
    }

    fn parent_inode(&self) -> Inode {
        *match self {
            Self::File(FileNode { parent_inode, .. }) => parent_inode,
            Self::ReadOnlyFile(ReadOnlyFileNode { parent_inode, .. }) => parent_inode,
            Self::CustomFile(CustomFileNode { parent_inode, .. }) => parent_inode,
            Self::Directory(DirectoryNode { parent_inode, .. }) => parent_inode,
            Self::ArcDirectory(ArcDirectoryNode { parent_inode, .. }) => parent_inode,
        }
    }
    fn set_parent_inode(&mut self, parent_inode: Inode) {
        match self {
            Self::File(f) => {
                f.parent_inode = parent_inode;
            }
            Self::ReadOnlyFile(f) => {
                f.parent_inode = parent_inode;
            }
            Self::CustomFile(f) => {
                f.parent_inode = parent_inode;
            }
            Self::Directory(d) => {
                d.parent_inode = parent_inode;
            }
            Self::ArcDirectory(d) => {
                d.parent_inode = parent_inode;
            }
        }
    }

    fn name(&self) -> &OsStr {
        match self {
            Self::File(FileNode { name, .. }) => name.as_os_str(),
            Self::ReadOnlyFile(ReadOnlyFileNode { name, .. }) => name.as_os_str(),
            Self::CustomFile(CustomFileNode { name, .. }) => name.as_os_str(),
            Self::Directory(DirectoryNode { name, .. }) => name.as_os_str(),
            Self::ArcDirectory(ArcDirectoryNode { name, .. }) => name.as_os_str(),
        }
    }

    fn metadata(&self) -> &Metadata {
        match self {
            Self::File(FileNode { metadata, .. }) => metadata,
            Self::ReadOnlyFile(ReadOnlyFileNode { metadata, .. }) => metadata,
            Self::CustomFile(CustomFileNode { metadata, .. }) => metadata,
            Self::Directory(DirectoryNode { metadata, .. }) => metadata,
            Self::ArcDirectory(ArcDirectoryNode { metadata, .. }) => metadata,
        }
    }

    fn metadata_mut(&mut self) -> &mut Metadata {
        match self {
            Self::File(FileNode { metadata, .. }) => metadata,
            Self::ReadOnlyFile(ReadOnlyFileNode { metadata, .. }) => metadata,
            Self::CustomFile(CustomFileNode { metadata, .. }) => metadata,
            Self::Directory(DirectoryNode { metadata, .. }) => metadata,
            Self::ArcDirectory(ArcDirectoryNode { metadata, .. }) => metadata,
        }
    }

    fn set_name(&mut self, new_name: OsString) {
        match self {
            Self::File(FileNode { name, .. }) => *name = new_name,
            Self::ReadOnlyFile(ReadOnlyFileNode { name, .. }) => *name = new_name,
            Self::CustomFile(CustomFileNode { name, .. }) => *name = new_name,
            Self::Directory(DirectoryNode { name, .. }) => *name = new_name,
            Self::ArcDirectory(ArcDirectoryNode { name, .. }) => *name = new_name,
        }
    }
}

fn time() -> u64 {
    #[cfg(not(feature = "no-time"))]
    {
        // SAFETY: It's very unlikely that the system returns a time that
        // is before `UNIX_EPOCH` :-).
        std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    #[cfg(feature = "no-time")]
    {
        0
    }
}
