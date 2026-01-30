//! Core identifier types.

/// Identifier for a mounted filesystem instance (mount namespace id).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct MountId(pub u32);

/// VFS handle identifier (typically refers to an open file description).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct VfsHandleId(pub u64);

/// VFS inode identity (per-mount inode namespace).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct VfsInodeId {
    pub mount: MountId,
    pub backend: u64,
}
