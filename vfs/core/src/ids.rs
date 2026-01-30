//! Core identifier types.

/// Identifier for a mounted filesystem instance (mount namespace id).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct MountId(pub u32);

/// Backend-defined inode identity (stable for the lifetime of the mount).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct BackendInodeId(pub u64);

/// VFS handle identifier (typically refers to an open file description).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct VfsHandleId(pub u64);

/// VFS inode identity (per-mount inode namespace).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct VfsInodeId {
    pub mount: MountId,
    pub backend: BackendInodeId,
}
