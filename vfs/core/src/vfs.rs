//! `Vfs` service object and public API shape.
//!
//! The implementations are filled in over later phases, but the method signatures are defined
//! early so `lib/wasix` can call into `vfs-core` without signature churn.

use crate::{
    DirStreamHandle, MkdirOptions, OpenOptions, ReadDirOptions, ReadlinkOptions, RenameOptions,
    StatOptions, SymlinkOptions, UnlinkOptions, VfsContext, VfsDirHandle, VfsError, VfsErrorKind,
    VfsHandle, VfsMetadata, VfsPath, VfsPathBuf, VfsResult,
};
use std::sync::Arc;

#[derive(Clone)]
pub struct Vfs {
    _inner: Arc<VfsInner>,
}

struct VfsInner;

impl Default for Vfs {
    fn default() -> Self {
        Self {
            _inner: Arc::new(VfsInner),
        }
    }
}

impl Vfs {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn openat(
        &self,
        _ctx: &VfsContext,
        _base: VfsBaseDir<'_>,
        _path: &VfsPath,
        _opts: OpenOptions,
    ) -> VfsResult<VfsHandle> {
        Err(VfsError::new(VfsErrorKind::NotImplemented, "vfs.openat"))
    }

    pub fn statat(
        &self,
        _ctx: &VfsContext,
        _base: VfsBaseDir<'_>,
        _path: &VfsPath,
        _opts: StatOptions,
    ) -> VfsResult<VfsMetadata> {
        Err(VfsError::new(VfsErrorKind::NotImplemented, "vfs.statat"))
    }

    pub fn mkdirat(
        &self,
        _ctx: &VfsContext,
        _base: VfsBaseDir<'_>,
        _path: &VfsPath,
        _opts: MkdirOptions,
    ) -> VfsResult<()> {
        Err(VfsError::new(VfsErrorKind::NotImplemented, "vfs.mkdirat"))
    }

    pub fn unlinkat(
        &self,
        _ctx: &VfsContext,
        _base: VfsBaseDir<'_>,
        _path: &VfsPath,
        _opts: UnlinkOptions,
    ) -> VfsResult<()> {
        Err(VfsError::new(VfsErrorKind::NotImplemented, "vfs.unlinkat"))
    }

    pub fn renameat(
        &self,
        _ctx: &VfsContext,
        _base_old: VfsBaseDir<'_>,
        _old_path: &VfsPath,
        _base_new: VfsBaseDir<'_>,
        _new_path: &VfsPath,
        _opts: RenameOptions,
    ) -> VfsResult<()> {
        Err(VfsError::new(VfsErrorKind::NotImplemented, "vfs.renameat"))
    }

    pub fn readlinkat(
        &self,
        _ctx: &VfsContext,
        _base: VfsBaseDir<'_>,
        _path: &VfsPath,
        _opts: ReadlinkOptions,
    ) -> VfsResult<VfsPathBuf> {
        Err(VfsError::new(
            VfsErrorKind::NotImplemented,
            "vfs.readlinkat",
        ))
    }

    pub fn symlinkat(
        &self,
        _ctx: &VfsContext,
        _base: VfsBaseDir<'_>,
        _link_path: &VfsPath,
        _target: &VfsPath,
        _opts: SymlinkOptions,
    ) -> VfsResult<()> {
        Err(VfsError::new(VfsErrorKind::NotImplemented, "vfs.symlinkat"))
    }

    pub fn readdir(
        &self,
        _ctx: &VfsContext,
        _dir: &VfsDirHandle,
        _opts: ReadDirOptions,
    ) -> VfsResult<DirStreamHandle> {
        Err(VfsError::new(VfsErrorKind::NotImplemented, "vfs.readdir"))
    }
}

/// Base directory for "at"-style operations.
pub enum VfsBaseDir<'a> {
    /// Resolve relative paths against `ctx.cwd`.
    Cwd,
    /// Resolve relative paths against this directory handle.
    Handle(&'a VfsDirHandle),
}
