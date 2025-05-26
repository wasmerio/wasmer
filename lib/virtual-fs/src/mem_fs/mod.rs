mod file;
mod file_opener;
mod filesystem;
mod offloaded_file;
mod stdio;

use file::{File, FileHandle, ReadOnlyFile};
pub use filesystem::FileSystem;
pub use offloaded_file::OffloadBackingStore;
#[cfg(not(feature = "js"))]
use std::time::{SystemTime, UNIX_EPOCH};
pub use stdio::{Stderr, Stdin, Stdout};
#[cfg(feature = "js")]
pub use web_time::{SystemTime, UNIX_EPOCH};

use crate::Metadata;
use std::{
    ffi::{OsStr, OsString},
    path::PathBuf,
    sync::{Arc, Mutex},
};

use self::offloaded_file::OffloadedFile;

type Inode = usize;
const ROOT_INODE: Inode = 0;

#[derive(Debug)]
struct FileNode {
    inode: Inode,
    name: OsString,
    file: File,
    metadata: Metadata,
}

#[derive(Debug)]
struct ReadOnlyFileNode {
    inode: Inode,
    name: OsString,
    file: ReadOnlyFile,
    metadata: Metadata,
}

#[derive(Debug)]
struct OffloadedFileNode {
    inode: Inode,
    name: OsString,
    file: OffloadedFile,
    metadata: Metadata,
}

#[derive(Debug)]
struct ArcFileNode {
    inode: Inode,
    name: OsString,
    fs: Arc<dyn crate::FileSystem + Send + Sync>,
    path: PathBuf,
    metadata: Metadata,
}

// FIXME: this is broken!!! A `VirtualFile` stores its own offset,
// so a file stored this way can only be read once!
#[derive(Debug)]
struct CustomFileNode {
    inode: Inode,
    name: OsString,
    file: Mutex<Box<dyn crate::VirtualFile + Send + Sync>>,
    metadata: Metadata,
}

#[derive(Debug)]
struct DirectoryNode {
    inode: Inode,
    name: OsString,
    children: Vec<Inode>,
    metadata: Metadata,
}

#[derive(Debug)]
struct ArcDirectoryNode {
    inode: Inode,
    name: OsString,
    fs: Arc<dyn crate::FileSystem + Send + Sync>,
    path: PathBuf,
    metadata: Metadata,
}

#[derive(Debug)]
enum Node {
    File(FileNode),
    OffloadedFile(OffloadedFileNode),
    ReadOnlyFile(ReadOnlyFileNode),
    ArcFile(ArcFileNode),
    CustomFile(CustomFileNode),
    Directory(DirectoryNode),
    ArcDirectory(ArcDirectoryNode),
}

impl Node {
    fn inode(&self) -> Inode {
        *match self {
            Self::File(FileNode { inode, .. }) => inode,
            Self::OffloadedFile(OffloadedFileNode { inode, .. }) => inode,
            Self::ReadOnlyFile(ReadOnlyFileNode { inode, .. }) => inode,
            Self::ArcFile(ArcFileNode { inode, .. }) => inode,
            Self::CustomFile(CustomFileNode { inode, .. }) => inode,
            Self::Directory(DirectoryNode { inode, .. }) => inode,
            Self::ArcDirectory(ArcDirectoryNode { inode, .. }) => inode,
        }
    }

    fn name(&self) -> &OsStr {
        match self {
            Self::File(FileNode { name, .. }) => name.as_os_str(),
            Self::OffloadedFile(OffloadedFileNode { name, .. }) => name.as_os_str(),
            Self::ReadOnlyFile(ReadOnlyFileNode { name, .. }) => name.as_os_str(),
            Self::ArcFile(ArcFileNode { name, .. }) => name.as_os_str(),
            Self::CustomFile(CustomFileNode { name, .. }) => name.as_os_str(),
            Self::Directory(DirectoryNode { name, .. }) => name.as_os_str(),
            Self::ArcDirectory(ArcDirectoryNode { name, .. }) => name.as_os_str(),
        }
    }

    fn metadata(&self) -> &Metadata {
        match self {
            Self::File(FileNode { metadata, .. }) => metadata,
            Self::OffloadedFile(OffloadedFileNode { metadata, .. }) => metadata,
            Self::ReadOnlyFile(ReadOnlyFileNode { metadata, .. }) => metadata,
            Self::ArcFile(ArcFileNode { metadata, .. }) => metadata,
            Self::CustomFile(CustomFileNode { metadata, .. }) => metadata,
            Self::Directory(DirectoryNode { metadata, .. }) => metadata,
            Self::ArcDirectory(ArcDirectoryNode { metadata, .. }) => metadata,
        }
    }

    fn metadata_mut(&mut self) -> &mut Metadata {
        match self {
            Self::File(FileNode { metadata, .. }) => metadata,
            Self::OffloadedFile(OffloadedFileNode { metadata, .. }) => metadata,
            Self::ReadOnlyFile(ReadOnlyFileNode { metadata, .. }) => metadata,
            Self::ArcFile(ArcFileNode { metadata, .. }) => metadata,
            Self::CustomFile(CustomFileNode { metadata, .. }) => metadata,
            Self::Directory(DirectoryNode { metadata, .. }) => metadata,
            Self::ArcDirectory(ArcDirectoryNode { metadata, .. }) => metadata,
        }
    }

    fn set_name(&mut self, new_name: OsString) {
        match self {
            Self::File(FileNode { name, .. }) => *name = new_name,
            Self::OffloadedFile(OffloadedFileNode { name, .. }) => *name = new_name,
            Self::ReadOnlyFile(ReadOnlyFileNode { name, .. }) => *name = new_name,
            Self::ArcFile(ArcFileNode { name, .. }) => *name = new_name,
            Self::CustomFile(CustomFileNode { name, .. }) => *name = new_name,
            Self::Directory(DirectoryNode { name, .. }) => *name = new_name,
            Self::ArcDirectory(ArcDirectoryNode { name, .. }) => *name = new_name,
        }
    }
}

fn time() -> u64 {
    // SAFETY: It's very unlikely that the system returns a time that
    // is before `UNIX_EPOCH` :-).
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64
}
