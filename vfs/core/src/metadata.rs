//! Metadata types (`stat`-like).

use crate::{VfsFileType, VfsInodeId, VfsTimespec};
use bitflags::bitflags;

pub type VfsUid = u32;
pub type VfsGid = u32;
pub type VfsTimestamp = VfsTimespec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VfsFileMode(pub u32);

impl VfsFileMode {
    pub const fn bits(self) -> u32 {
        self.0
    }

    pub const fn owner_bits(self) -> u32 {
        (self.0 >> 6) & 0o7
    }

    pub const fn group_bits(self) -> u32 {
        (self.0 >> 3) & 0o7
    }

    pub const fn other_bits(self) -> u32 {
        self.0 & 0o7
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct VfsAccess: u8 {
        const READ = 1 << 0;
        const WRITE = 1 << 1;
        const EXEC = 1 << 2;
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VfsMetadata {
    pub inode: VfsInodeId,
    pub file_type: VfsFileType,
    pub mode: VfsFileMode,
    pub nlink: u64,
    pub uid: VfsUid,
    pub gid: VfsGid,
    pub size: u64,
    pub atime: VfsTimestamp,
    pub mtime: VfsTimestamp,
    pub ctime: VfsTimestamp,
    /// Backend-defined device id (for device nodes; otherwise usually 0).
    pub rdev_major: u32,
    pub rdev_minor: u32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct VfsSetMetadata {
    pub mode: Option<VfsFileMode>,
    pub uid: Option<VfsUid>,
    pub gid: Option<VfsGid>,
    pub size: Option<u64>,
    pub atime: Option<VfsTimestamp>,
    pub mtime: Option<VfsTimestamp>,
    pub ctime: Option<VfsTimestamp>,
}
