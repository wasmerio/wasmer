//! `Vfs` service object and public API shape.
//!
//! The implementations are filled in over later phases, but the method signatures are defined
//! early so `lib/wasix` can call into `vfs-core` without signature churn.

use crate::{
    DirStreamHandle, MkdirOptions, OpenOptions, ReadDirOptions, ReadlinkOptions, RenameOptions,
    StatOptions, SymlinkOptions, UnlinkOptions, VfsContext, VfsDirHandle, VfsDirHandleAsync,
    VfsError, VfsErrorKind, VfsHandle, VfsHandleAsync, VfsMetadata, VfsPath, VfsPathBuf, VfsResult,
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

    pub async fn openat_async(
        &self,
        _ctx: &VfsContext,
        _base: VfsBaseDirAsync<'_>,
        _path: &VfsPath,
        _opts: OpenOptions,
    ) -> VfsResult<VfsHandleAsync> {
        Err(VfsError::new(
            VfsErrorKind::NotImplemented,
            "vfs.openat_async",
        ))
    }

    pub async fn statat_async(
        &self,
        _ctx: &VfsContext,
        _base: VfsBaseDirAsync<'_>,
        _path: &VfsPath,
        _opts: StatOptions,
    ) -> VfsResult<VfsMetadata> {
        Err(VfsError::new(
            VfsErrorKind::NotImplemented,
            "vfs.statat_async",
        ))
    }

    pub async fn mkdirat_async(
        &self,
        _ctx: &VfsContext,
        _base: VfsBaseDirAsync<'_>,
        _path: &VfsPath,
        _opts: MkdirOptions,
    ) -> VfsResult<()> {
        Err(VfsError::new(
            VfsErrorKind::NotImplemented,
            "vfs.mkdirat_async",
        ))
    }

    pub async fn unlinkat_async(
        &self,
        _ctx: &VfsContext,
        _base: VfsBaseDirAsync<'_>,
        _path: &VfsPath,
        _opts: UnlinkOptions,
    ) -> VfsResult<()> {
        Err(VfsError::new(
            VfsErrorKind::NotImplemented,
            "vfs.unlinkat_async",
        ))
    }

    pub async fn renameat_async(
        &self,
        _ctx: &VfsContext,
        _base_old: VfsBaseDirAsync<'_>,
        _old_path: &VfsPath,
        _base_new: VfsBaseDirAsync<'_>,
        _new_path: &VfsPath,
        _opts: RenameOptions,
    ) -> VfsResult<()> {
        Err(VfsError::new(
            VfsErrorKind::NotImplemented,
            "vfs.renameat_async",
        ))
    }

    pub async fn readlinkat_async(
        &self,
        _ctx: &VfsContext,
        _base: VfsBaseDirAsync<'_>,
        _path: &VfsPath,
        _opts: ReadlinkOptions,
    ) -> VfsResult<VfsPathBuf> {
        Err(VfsError::new(
            VfsErrorKind::NotImplemented,
            "vfs.readlinkat_async",
        ))
    }

    pub async fn symlinkat_async(
        &self,
        _ctx: &VfsContext,
        _base: VfsBaseDirAsync<'_>,
        _link_path: &VfsPath,
        _target: &VfsPath,
        _opts: SymlinkOptions,
    ) -> VfsResult<()> {
        Err(VfsError::new(
            VfsErrorKind::NotImplemented,
            "vfs.symlinkat_async",
        ))
    }

    pub async fn readdir_async(
        &self,
        _ctx: &VfsContext,
        _dir: &VfsDirHandleAsync,
        _opts: ReadDirOptions,
    ) -> VfsResult<DirStreamHandle> {
        Err(VfsError::new(
            VfsErrorKind::NotImplemented,
            "vfs.readdir_async",
        ))
    }
}

/// Base directory for "at"-style operations.
pub enum VfsBaseDir<'a> {
    /// Resolve relative paths against `ctx.cwd`.
    Cwd,
    /// Resolve relative paths against this directory handle.
    Handle(&'a VfsDirHandle),
}

/// Base directory for async "at"-style operations.
pub enum VfsBaseDirAsync<'a> {
    /// Resolve relative paths against `ctx.cwd_async` when set.
    Cwd,
    /// Resolve relative paths against this directory handle.
    Handle(&'a VfsDirHandleAsync),
}
