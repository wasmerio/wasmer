//! `Vfs` service object and public API shape.
//!
//! The implementations are filled in over later phases, but the method signatures are defined
//! early so `lib/wasix` can call into `vfs-core` without signature churn.

use crate::inode::{NodeRef, NodeRefAsync, make_vfs_inode};
use crate::node::{
    CreateFile, MkdirOptions as NodeMkdirOptions, ReadDirBatch, RenameOptions as NodeRenameOptions,
    UnlinkOptions as NodeUnlinkOptions, VfsDirCookie,
};
use crate::path_walker::{PathWalker, PathWalkerAsync, ResolutionRequest, ResolutionRequestAsync};
use crate::policy::VfsMutationOp;
use crate::mount::MountTable;
use crate::{
    DirStreamHandle, MkdirOptions, MountId, OpenFlags, OpenOptions, ReadDirOptions, ReadlinkOptions,
    RenameOptions, ResolveFlags, StatOptions, SymlinkOptions, UnlinkOptions, VfsContext,
    VfsDirHandle, VfsDirHandleAsync, VfsError, VfsErrorKind, VfsFileType, VfsHandle,
    VfsHandleAsync, VfsHandleId, VfsMetadata, VfsPath, VfsPathBuf, VfsResult, WalkFlags,
};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use vfs_ratelimit::LimiterChain;

#[derive(Clone)]
pub struct Vfs {
    inner: Arc<VfsInner>,
}

struct VfsInner {
    mount_table: Arc<MountTable>,
    next_handle_id: AtomicU64,
    dir_streams: Mutex<HashMap<VfsHandleId, DirStreamState>>,
}

impl Vfs {
    pub fn new(mount_table: Arc<MountTable>) -> Self {
        Self {
            inner: Arc::new(VfsInner {
                mount_table,
                next_handle_id: AtomicU64::new(1),
                dir_streams: Mutex::new(HashMap::new()),
            }),
        }
    }

    fn alloc_handle_id(&self) -> VfsHandleId {
        VfsHandleId(self.inner.next_handle_id.fetch_add(1, Ordering::Relaxed))
    }

    fn limiter_chain(&self, ctx: &VfsContext, mount: MountId) -> LimiterChain {
        let mut chain = LimiterChain::default();
        chain.global = ctx.rate_limiter.clone();
        let inner = self.inner.mount_table.snapshot();
        if let Some(entry) = inner.mounts.get(mount.index()).and_then(|slot| slot.as_ref()) {
            chain.mount = entry.mount_limiter.clone();
            chain.fs = entry.fs_limiter.clone();
        }
        chain
    }

    fn path_walker(&self) -> PathWalker {
        PathWalker::new(self.inner.mount_table.clone())
    }

    fn path_walker_async(&self) -> PathWalkerAsync {
        PathWalkerAsync::new(self.inner.mount_table.clone())
    }

    fn walk_flags(ctx: &VfsContext, resolve: ResolveFlags) -> WalkFlags {
        let mut walk = WalkFlags::new(ctx);
        if resolve.contains(ResolveFlags::NO_SYMLINK_FOLLOW) {
            walk.follow_final_symlink = false;
        }
        if resolve.contains(ResolveFlags::BENEATH) {
            walk.resolve_beneath = true;
        }
        if resolve.contains(ResolveFlags::IN_ROOT) {
            walk.in_root = true;
        }
        walk
    }

    fn name_from_buf(name: &crate::VfsNameBuf) -> VfsResult<crate::VfsName<'_>> {
        crate::VfsName::new(name.as_bytes())
            .map_err(|_| VfsError::new(VfsErrorKind::Internal, "vfs.name"))
    }

    pub fn openat(
        &self,
        ctx: &VfsContext,
        base: VfsBaseDir<'_>,
        path: &VfsPath,
        opts: OpenOptions,
    ) -> VfsResult<VfsHandle> {
        let walk = Self::walk_flags(ctx, opts.resolve);
        let flags = opts.flags;
        let walker = self.path_walker();

        let (mount, inode, node) = if flags.contains(OpenFlags::CREATE) {
            let parent = walker.resolve_parent(ResolutionRequest {
                ctx,
                base,
                path,
                flags: walk,
            })?;
            let parent_meta = parent.dir.node.metadata()?;
            ctx.policy
                .check_mutation(ctx, &parent_meta, VfsMutationOp::CreateFile)?;
            let name = Self::name_from_buf(&parent.name)?;
            let node = parent.dir.node.create_file(
                &name,
                CreateFile {
                    mode: opts.mode,
                    truncate: flags.contains(OpenFlags::TRUNC),
                    exclusive: flags.contains(OpenFlags::EXCL),
                },
            )?;
            let inode = make_vfs_inode(parent.dir.mount, node.inode());
            (parent.dir.mount, inode, node)
        } else {
            let resolved = walker.resolve(ResolutionRequest {
                ctx,
                base,
                path,
                flags: walk,
            })?;
            if resolved.node.file_type() == VfsFileType::Directory {
                return Err(VfsError::new(VfsErrorKind::IsDir, "vfs.openat"));
            }
            (resolved.mount, resolved.inode, resolved.node)
        };

        if node.file_type() == VfsFileType::Directory {
            return Err(VfsError::new(VfsErrorKind::IsDir, "vfs.openat"));
        }
        let meta = node.metadata()?;
        ctx.policy.check_open(ctx, &meta, flags)?;
        let backend = node.open(opts)?;
        let guard = self.inner.mount_table.guard(mount)?;
        let handle_id = self.alloc_handle_id();
        let limiter_chain = self.limiter_chain(ctx, mount);
        Ok(VfsHandle::new(
            handle_id,
            guard,
            inode,
            node.file_type(),
            backend,
            flags,
            limiter_chain,
        ))
    }

    pub fn statat(
        &self,
        ctx: &VfsContext,
        base: VfsBaseDir<'_>,
        path: &VfsPath,
        opts: StatOptions,
    ) -> VfsResult<VfsMetadata> {
        let walk = Self::walk_flags(ctx, opts.resolve);
        if !opts.follow {
            walk.follow_final_symlink = false;
        }
        if opts.require_dir_if_trailing_slash {
            walk.must_be_dir = true;
        }
        let resolved = self.path_walker().resolve(ResolutionRequest {
            ctx,
            base,
            path,
            flags: walk,
        })?;
        resolved.node.metadata()
    }

    pub fn mkdirat(
        &self,
        ctx: &VfsContext,
        base: VfsBaseDir<'_>,
        path: &VfsPath,
        opts: MkdirOptions,
    ) -> VfsResult<()> {
        let walk = Self::walk_flags(ctx, opts.resolve);
        let parent = self.path_walker().resolve_parent(ResolutionRequest {
            ctx,
            base,
            path,
            flags: walk,
        })?;
        let parent_meta = parent.dir.node.metadata()?;
        ctx.policy
            .check_mutation(ctx, &parent_meta, VfsMutationOp::CreateDir)?;
        let name = Self::name_from_buf(&parent.name)?;
        parent
            .dir
            .node
            .mkdir(&name, NodeMkdirOptions { mode: opts.mode })?;
        Ok(())
    }

    pub fn unlinkat(
        &self,
        ctx: &VfsContext,
        base: VfsBaseDir<'_>,
        path: &VfsPath,
        opts: UnlinkOptions,
    ) -> VfsResult<()> {
        let walk = Self::walk_flags(ctx, opts.resolve);
        let parent = self.path_walker().resolve_parent(ResolutionRequest {
            ctx,
            base,
            path,
            flags: walk,
        })?;
        let parent_meta = parent.dir.node.metadata()?;
        ctx.policy
            .check_mutation(ctx, &parent_meta, VfsMutationOp::Remove { is_dir: false })?;
        let name = Self::name_from_buf(&parent.name)?;
        parent.dir.node.unlink(
            &name,
            NodeUnlinkOptions {
                must_be_dir: false,
            },
        )?;
        Ok(())
    }

    pub fn renameat(
        &self,
        ctx: &VfsContext,
        base_old: VfsBaseDir<'_>,
        old_path: &VfsPath,
        base_new: VfsBaseDir<'_>,
        new_path: &VfsPath,
        opts: RenameOptions,
    ) -> VfsResult<()> {
        let walk = Self::walk_flags(ctx, opts.resolve);
        let walker = self.path_walker();
        let old_parent = walker.resolve_parent(ResolutionRequest {
            ctx,
            base: base_old,
            path: old_path,
            flags: walk,
        })?;
        let new_parent = walker.resolve_parent(ResolutionRequest {
            ctx,
            base: base_new,
            path: new_path,
            flags: walk,
        })?;
        if old_parent.dir.mount != new_parent.dir.mount {
            return Err(VfsError::new(VfsErrorKind::CrossDevice, "vfs.renameat"));
        }
        let old_parent_meta = old_parent.dir.node.metadata()?;
        ctx.policy
            .check_mutation(ctx, &old_parent_meta, VfsMutationOp::Rename)?;
        let new_parent_meta = new_parent.dir.node.metadata()?;
        ctx.policy
            .check_mutation(ctx, &new_parent_meta, VfsMutationOp::Rename)?;
        let old_name = Self::name_from_buf(&old_parent.name)?;
        let new_name = Self::name_from_buf(&new_parent.name)?;
        old_parent.dir.node.rename(
            &old_name,
            new_parent.dir.node.as_ref(),
            &new_name,
            NodeRenameOptions {
                noreplace: opts.flags.contains(crate::RenameFlags::NOREPLACE),
                exchange: opts.flags.contains(crate::RenameFlags::EXCHANGE),
            },
        )?;
        Ok(())
    }

    pub fn readlinkat(
        &self,
        ctx: &VfsContext,
        base: VfsBaseDir<'_>,
        path: &VfsPath,
        opts: ReadlinkOptions,
    ) -> VfsResult<VfsPathBuf> {
        let mut walk = Self::walk_flags(ctx, opts.resolve);
        walk.follow_final_symlink = false;
        let resolved = self.path_walker().resolve(ResolutionRequest {
            ctx,
            base,
            path,
            flags: walk,
        })?;
        if resolved.node.file_type() != VfsFileType::Symlink {
            return Err(VfsError::new(VfsErrorKind::InvalidInput, "vfs.readlinkat"));
        }
        resolved.node.readlink()
    }

    pub fn symlinkat(
        &self,
        ctx: &VfsContext,
        base: VfsBaseDir<'_>,
        link_path: &VfsPath,
        target: &VfsPath,
        opts: SymlinkOptions,
    ) -> VfsResult<()> {
        let walk = Self::walk_flags(ctx, opts.resolve);
        let parent = self.path_walker().resolve_parent(ResolutionRequest {
            ctx,
            base,
            path: link_path,
            flags: walk,
        })?;
        let parent_meta = parent.dir.node.metadata()?;
        ctx.policy
            .check_mutation(ctx, &parent_meta, VfsMutationOp::Symlink)?;
        let name = Self::name_from_buf(&parent.name)?;
        parent.dir.node.symlink(&name, target)?;
        Ok(())
    }

    pub fn readdir(
        &self,
        _ctx: &VfsContext,
        dir: &VfsDirHandle,
        _opts: ReadDirOptions,
    ) -> VfsResult<DirStreamHandle> {
        let id = self.alloc_handle_id();
        let mut streams = self.inner.dir_streams.lock();
        streams.insert(
            id,
            DirStreamState::Sync(DirStreamStateSync {
                dir: dir.clone(),
                cursor: None,
                finished: false,
            }),
        );
        Ok(DirStreamHandle::new(id))
    }

    pub fn readdir_next(
        &self,
        _ctx: &VfsContext,
        stream: &DirStreamHandle,
        max: usize,
    ) -> VfsResult<ReadDirBatch> {
        let (dir, cursor, finished) = {
            let streams = self.inner.dir_streams.lock();
            let state = streams
                .get(&stream.id())
                .ok_or_else(|| VfsError::new(VfsErrorKind::BadHandle, "vfs.readdir_next"))?;
            let DirStreamState::Sync(state) = state else {
                return Err(VfsError::new(VfsErrorKind::BadHandle, "vfs.readdir_next"));
            };
            (state.dir.clone(), state.cursor, state.finished)
        };
        if finished {
            return Ok(ReadDirBatch {
                entries: Default::default(),
                next: None,
            });
        }
        let batch = dir.node().read_dir(cursor, max)?;
        let finished = batch.next.is_none();
        let mut streams = self.inner.dir_streams.lock();
        let state = streams
            .get_mut(&stream.id())
            .ok_or_else(|| VfsError::new(VfsErrorKind::BadHandle, "vfs.readdir_next"))?;
        let DirStreamState::Sync(state) = state else {
            return Err(VfsError::new(VfsErrorKind::BadHandle, "vfs.readdir_next"));
        };
        state.cursor = batch.next;
        state.finished = finished;
        Ok(batch)
    }

    pub fn readdir_close(&self, stream: DirStreamHandle) -> VfsResult<()> {
        let mut streams = self.inner.dir_streams.lock();
        if streams.remove(&stream.id()).is_none() {
            return Err(VfsError::new(VfsErrorKind::BadHandle, "vfs.readdir_close"));
        }
        Ok(())
    }

    pub fn opendirat(
        &self,
        ctx: &VfsContext,
        base: VfsBaseDir<'_>,
        path: &VfsPath,
        opts: OpenOptions,
    ) -> VfsResult<VfsDirHandle> {
        if !opts.flags.contains(OpenFlags::DIRECTORY)
            || opts
                .flags
                .intersects(OpenFlags::TRUNC | OpenFlags::CREATE | OpenFlags::EXCL | OpenFlags::APPEND)
        {
            return Err(VfsError::new(VfsErrorKind::InvalidInput, "vfs.opendirat"));
        }
        let mut walk = Self::walk_flags(ctx, opts.resolve);
        walk.must_be_dir = true;
        let resolved = self.path_walker().resolve(ResolutionRequest {
            ctx,
            base,
            path,
            flags: walk,
        })?;
        let guard = self.inner.mount_table.guard(resolved.mount)?;
        let handle_id = self.alloc_handle_id();
        let parent = resolved.parent.as_ref().map(|parent| {
            NodeRef::new(parent.dir.mount, parent.dir.node.clone())
        });
        Ok(VfsDirHandle::new(
            handle_id,
            guard,
            resolved.inode,
            resolved.node,
            parent,
        ))
    }

    pub async fn openat_async(
        &self,
        ctx: &VfsContext,
        base: VfsBaseDirAsync<'_>,
        path: &VfsPath,
        opts: OpenOptions,
    ) -> VfsResult<VfsHandleAsync> {
        let mut walk = Self::walk_flags(ctx, opts.resolve);
        let flags = opts.flags;
        let walker = self.path_walker_async();

        let (mount, inode, node) = if flags.contains(OpenFlags::CREATE) {
            let parent = walker
                .resolve_parent(ResolutionRequestAsync {
                    ctx,
                    base,
                    path,
                    flags: walk,
                })
                .await?;
            let parent_meta = parent.dir.node.metadata().await?;
            ctx.policy
                .check_mutation(ctx, &parent_meta, VfsMutationOp::CreateFile)?;
            let name = Self::name_from_buf(&parent.name)?;
            let node = parent
                .dir
                .node
                .create_file(
                    &name,
                    CreateFile {
                        mode: opts.mode,
                        truncate: flags.contains(OpenFlags::TRUNC),
                        exclusive: flags.contains(OpenFlags::EXCL),
                    },
                )
                .await?;
            let inode = make_vfs_inode(parent.dir.mount, node.inode());
            (parent.dir.mount, inode, node)
        } else {
            let resolved = walker
                .resolve(ResolutionRequestAsync {
                    ctx,
                    base,
                    path,
                    flags: walk,
                })
                .await?;
            if resolved.node.file_type() == VfsFileType::Directory {
                return Err(VfsError::new(VfsErrorKind::IsDir, "vfs.openat_async"));
            }
            (resolved.mount, resolved.inode, resolved.node)
        };

        if node.file_type() == VfsFileType::Directory {
            return Err(VfsError::new(VfsErrorKind::IsDir, "vfs.openat_async"));
        }
        let meta = node.metadata().await?;
        ctx.policy.check_open(ctx, &meta, flags)?;
        let backend = node.open(opts).await?;
        let guard = self.inner.mount_table.guard(mount)?;
        let handle_id = self.alloc_handle_id();
        let limiter_chain = self.limiter_chain(ctx, mount);
        Ok(VfsHandleAsync::new(
            handle_id,
            guard,
            inode,
            node.file_type(),
            backend,
            flags,
            limiter_chain,
        ))
    }

    pub async fn statat_async(
        &self,
        ctx: &VfsContext,
        base: VfsBaseDirAsync<'_>,
        path: &VfsPath,
        opts: StatOptions,
    ) -> VfsResult<VfsMetadata> {
        let mut walk = Self::walk_flags(ctx, opts.resolve);
        if !opts.follow {
            walk.follow_final_symlink = false;
        }
        if opts.require_dir_if_trailing_slash {
            walk.must_be_dir = true;
        }
        let resolved = self
            .path_walker_async()
            .resolve(ResolutionRequestAsync {
                ctx,
                base,
                path,
                flags: walk,
            })
            .await?;
        resolved.node.metadata().await
    }

    pub async fn mkdirat_async(
        &self,
        ctx: &VfsContext,
        base: VfsBaseDirAsync<'_>,
        path: &VfsPath,
        opts: MkdirOptions,
    ) -> VfsResult<()> {
        let walk = Self::walk_flags(ctx, opts.resolve);
        let parent = self
            .path_walker_async()
            .resolve_parent(ResolutionRequestAsync {
                ctx,
                base,
                path,
                flags: walk,
            })
            .await?;
        let parent_meta = parent.dir.node.metadata().await?;
        ctx.policy
            .check_mutation(ctx, &parent_meta, VfsMutationOp::CreateDir)?;
        let name = Self::name_from_buf(&parent.name)?;
        parent
            .dir
            .node
            .mkdir(&name, NodeMkdirOptions { mode: opts.mode })
            .await?;
        Ok(())
    }

    pub async fn unlinkat_async(
        &self,
        ctx: &VfsContext,
        base: VfsBaseDirAsync<'_>,
        path: &VfsPath,
        opts: UnlinkOptions,
    ) -> VfsResult<()> {
        let walk = Self::walk_flags(ctx, opts.resolve);
        let parent = self
            .path_walker_async()
            .resolve_parent(ResolutionRequestAsync {
                ctx,
                base,
                path,
                flags: walk,
            })
            .await?;
        let parent_meta = parent.dir.node.metadata().await?;
        ctx.policy
            .check_mutation(ctx, &parent_meta, VfsMutationOp::Remove { is_dir: false })?;
        let name = Self::name_from_buf(&parent.name)?;
        parent
            .dir
            .node
            .unlink(
                &name,
                NodeUnlinkOptions {
                    must_be_dir: false,
                },
            )
            .await?;
        Ok(())
    }

    pub async fn renameat_async(
        &self,
        ctx: &VfsContext,
        base_old: VfsBaseDirAsync<'_>,
        old_path: &VfsPath,
        base_new: VfsBaseDirAsync<'_>,
        new_path: &VfsPath,
        opts: RenameOptions,
    ) -> VfsResult<()> {
        let walk = Self::walk_flags(ctx, opts.resolve);
        let walker = self.path_walker_async();
        let old_parent = walker
            .resolve_parent(ResolutionRequestAsync {
                ctx,
                base: base_old,
                path: old_path,
                flags: walk,
            })
            .await?;
        let new_parent = walker
            .resolve_parent(ResolutionRequestAsync {
                ctx,
                base: base_new,
                path: new_path,
                flags: walk,
            })
            .await?;
        if old_parent.dir.mount != new_parent.dir.mount {
            return Err(VfsError::new(VfsErrorKind::CrossDevice, "vfs.renameat_async"));
        }
        let old_parent_meta = old_parent.dir.node.metadata().await?;
        ctx.policy
            .check_mutation(ctx, &old_parent_meta, VfsMutationOp::Rename)?;
        let new_parent_meta = new_parent.dir.node.metadata().await?;
        ctx.policy
            .check_mutation(ctx, &new_parent_meta, VfsMutationOp::Rename)?;
        let old_name = Self::name_from_buf(&old_parent.name)?;
        let new_name = Self::name_from_buf(&new_parent.name)?;
        old_parent
            .dir
            .node
            .rename(
                &old_name,
                new_parent.dir.node.as_ref(),
                &new_name,
                NodeRenameOptions {
                    noreplace: opts.flags.contains(crate::RenameFlags::NOREPLACE),
                    exchange: opts.flags.contains(crate::RenameFlags::EXCHANGE),
                },
            )
            .await?;
        Ok(())
    }

    pub async fn readlinkat_async(
        &self,
        ctx: &VfsContext,
        base: VfsBaseDirAsync<'_>,
        path: &VfsPath,
        opts: ReadlinkOptions,
    ) -> VfsResult<VfsPathBuf> {
        let mut walk = Self::walk_flags(ctx, opts.resolve);
        walk.follow_final_symlink = false;
        let resolved = self
            .path_walker_async()
            .resolve(ResolutionRequestAsync {
                ctx,
                base,
                path,
                flags: walk,
            })
            .await?;
        if resolved.node.file_type() != VfsFileType::Symlink {
            return Err(VfsError::new(
                VfsErrorKind::InvalidInput,
                "vfs.readlinkat_async",
            ));
        }
        resolved.node.readlink().await
    }

    pub async fn symlinkat_async(
        &self,
        ctx: &VfsContext,
        base: VfsBaseDirAsync<'_>,
        link_path: &VfsPath,
        target: &VfsPath,
        opts: SymlinkOptions,
    ) -> VfsResult<()> {
        let walk = Self::walk_flags(ctx, opts.resolve);
        let parent = self
            .path_walker_async()
            .resolve_parent(ResolutionRequestAsync {
                ctx,
                base,
                path: link_path,
                flags: walk,
            })
            .await?;
        let parent_meta = parent.dir.node.metadata().await?;
        ctx.policy
            .check_mutation(ctx, &parent_meta, VfsMutationOp::Symlink)?;
        let name = Self::name_from_buf(&parent.name)?;
        parent.dir.node.symlink(&name, target).await?;
        Ok(())
    }

    pub async fn readdir_async(
        &self,
        _ctx: &VfsContext,
        dir: &VfsDirHandleAsync,
        _opts: ReadDirOptions,
    ) -> VfsResult<DirStreamHandle> {
        let id = self.alloc_handle_id();
        let mut streams = self.inner.dir_streams.lock();
        streams.insert(
            id,
            DirStreamState::Async(DirStreamStateAsync {
                dir: dir.clone(),
                cursor: None,
                finished: false,
            }),
        );
        Ok(DirStreamHandle::new(id))
    }

    pub async fn readdir_next_async(
        &self,
        _ctx: &VfsContext,
        stream: &DirStreamHandle,
        max: usize,
    ) -> VfsResult<ReadDirBatch> {
        let (dir, cursor, finished) = {
            let streams = self.inner.dir_streams.lock();
            let state = streams
                .get(&stream.id())
                .ok_or_else(|| VfsError::new(VfsErrorKind::BadHandle, "vfs.readdir_next_async"))?;
            let DirStreamState::Async(state) = state else {
                return Err(VfsError::new(VfsErrorKind::BadHandle, "vfs.readdir_next_async"));
            };
            (state.dir.clone(), state.cursor, state.finished)
        };
        if finished {
            return Ok(ReadDirBatch {
                entries: Default::default(),
                next: None,
            });
        }
        let batch = dir.node().read_dir(cursor, max).await?;
        let finished = batch.next.is_none();
        let mut streams = self.inner.dir_streams.lock();
        let state = streams
            .get_mut(&stream.id())
            .ok_or_else(|| VfsError::new(VfsErrorKind::BadHandle, "vfs.readdir_next_async"))?;
        let DirStreamState::Async(state) = state else {
            return Err(VfsError::new(VfsErrorKind::BadHandle, "vfs.readdir_next_async"));
        };
        state.cursor = batch.next;
        state.finished = finished;
        Ok(batch)
    }

    pub fn readdir_close_async(&self, stream: DirStreamHandle) -> VfsResult<()> {
        self.readdir_close(stream)
    }

    pub async fn opendirat_async(
        &self,
        ctx: &VfsContext,
        base: VfsBaseDirAsync<'_>,
        path: &VfsPath,
        opts: OpenOptions,
    ) -> VfsResult<VfsDirHandleAsync> {
        if !opts.flags.contains(OpenFlags::DIRECTORY)
            || opts
                .flags
                .intersects(OpenFlags::TRUNC | OpenFlags::CREATE | OpenFlags::EXCL | OpenFlags::APPEND)
        {
            return Err(VfsError::new(VfsErrorKind::InvalidInput, "vfs.opendirat_async"));
        }
        let mut walk = Self::walk_flags(ctx, opts.resolve);
        walk.must_be_dir = true;
        let resolved = self
            .path_walker_async()
            .resolve(ResolutionRequestAsync {
                ctx,
                base,
                path,
                flags: walk,
            })
            .await?;
        let guard = self.inner.mount_table.guard(resolved.mount)?;
        let handle_id = self.alloc_handle_id();
        let parent = resolved.parent.as_ref().map(|parent| {
            NodeRefAsync::new(parent.dir.mount, parent.dir.node.clone())
        });
        Ok(VfsDirHandleAsync::new(
            handle_id,
            guard,
            resolved.inode,
            resolved.node,
            parent,
        ))
    }
}

struct DirStreamStateSync {
    dir: VfsDirHandle,
    cursor: Option<VfsDirCookie>,
    finished: bool,
}

struct DirStreamStateAsync {
    dir: VfsDirHandleAsync,
    cursor: Option<VfsDirCookie>,
    finished: bool,
}

enum DirStreamState {
    Sync(DirStreamStateSync),
    Async(DirStreamStateAsync),
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
