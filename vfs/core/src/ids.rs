//! Core identifier types.

use core::num::{NonZeroU32, NonZeroU64};

/// Identifier for a mounted filesystem instance (mount namespace id).
///
/// `0` is reserved for "unset/invalid". Use [`MountId::new`] or
/// [`MountId::from_index`] to construct values.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct MountId(NonZeroU32);

impl MountId {
    /// Create a new mount id from a raw value (must be non-zero).
    #[inline]
    pub fn new(raw: u32) -> Option<Self> {
        NonZeroU32::new(raw).map(Self)
    }

    /// Create a mount id from a zero-based slot index.
    #[inline]
    pub fn from_index(index: usize) -> Self {
        let raw = (index as u32).saturating_add(1);
        Self(NonZeroU32::new(raw).expect("mount id must be non-zero"))
    }

    /// Get the raw mount id value.
    #[inline]
    pub fn get(self) -> u32 {
        self.0.get()
    }

    /// Convert a mount id to a zero-based slot index.
    #[inline]
    pub fn index(self) -> usize {
        self.0.get().saturating_sub(1) as usize
    }
}

/// Backend-defined inode identity (stable for the lifetime of the mount).
///
/// `0` is reserved for "unset/invalid". Use [`BackendInodeId::new`] to
/// construct values.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct BackendInodeId(NonZeroU64);

impl BackendInodeId {
    /// Create a new backend inode id from a raw value (must be non-zero).
    #[inline]
    pub fn new(raw: u64) -> Option<Self> {
        NonZeroU64::new(raw).map(Self)
    }

    /// Get the raw backend inode id value.
    #[inline]
    pub fn get(self) -> u64 {
        self.0.get()
    }
}

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
