//! Filesystem node interfaces.
//!
//! Backend-facing `FsNode` trait for mount-agnostic VFS operations.

use crate::VfsFileType;
use crate::dir::VfsDirEntry;
use crate::inode::make_vfs_inode;
use crate::{MountId, VfsInodeId, VfsSetMetadata};
use smallvec::SmallVec;
use std::any::Any;
use std::sync::Arc;

/// Minimal node type classification (alias for now).
pub type VfsNodeKind = VfsFileType;

#[derive(Clone, Copy, Debug, Default)]
pub struct CreateFile {
    pub mode: Option<u32>,
    pub truncate: bool,
    pub exclusive: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct MkdirOptions {
    pub mode: Option<u32>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct UnlinkOptions {
    pub must_be_dir: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RenameOptions {
    pub noreplace: bool,
    pub exchange: bool,
}

pub type SetMetadata = VfsSetMetadata;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct VfsDirCookie(pub u64);

pub type DirCursor = VfsDirCookie;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadDirBatch {
    pub entries: SmallVec<[VfsDirEntry; 16]>,
    pub next: Option<VfsDirCookie>,
}

pub use crate::traits_async::{FsHandleAsync, FsNodeAsync};
pub use crate::traits_sync::{FsHandleSync, FsNodeSync};
pub use crate::traits_sync::{FsHandleSync as FsHandle, FsNodeSync as FsNode};

pub trait AsAny {
    fn as_any(&self) -> &dyn Any;
}

impl<T> AsAny for T
where
    T: Any,
{
    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[derive(Clone)]
pub struct VfsNode {
    pub id: VfsInodeId,
    pub inner: Arc<dyn FsNode>,
}

impl VfsNode {
    pub fn new(mount: MountId, inner: Arc<dyn FsNode>) -> Self {
        Self {
            id: make_vfs_inode(mount, inner.inode()),
            inner,
        }
    }
}
