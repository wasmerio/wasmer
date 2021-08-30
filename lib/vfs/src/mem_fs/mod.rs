mod file;
mod file_opener;
mod filesystem;
mod stdio;

use file::{File, FileHandle};
pub use file_opener::FileOpener;
pub use filesystem::FileSystem;
pub use stdio::{Stderr, Stdin, Stdout};

use crate::Metadata;
use std::ffi::{OsStr, OsString};

type Inode = usize;
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

// If the `host-fs` feature is not enabled, let's write a
// `TryInto<i32>` implementation for `FileDescriptor`, otherwise on
// Unix, it conflicts with `TryInto<RawFd>` (where `RawFd` is an alias
// to `i32`).
#[cfg(not(all(unix, feature = "host-fs")))]
impl std::convert::TryInto<i32> for crate::FileDescriptor {
    type Error = crate::FsError;

    fn try_into(self) -> std::result::Result<i32, Self::Error> {
        self.0.try_into().map_err(|_| crate::FsError::InvalidFd)
    }
}
