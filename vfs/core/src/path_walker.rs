//! Mount-aware path traversal and resolution.

use crate::inode::{NodeRef, NodeRefAsync, make_vfs_inode};
use crate::mount::MountTable;
use crate::node::{FsNode, FsNodeAsync};
use crate::path_types::{VfsComponent, VfsName, VfsNameBuf, VfsPath, VfsPathBuf};
use crate::{
    VfsBaseDir, VfsBaseDirAsync, VfsContext, VfsError, VfsErrorKind, VfsFileType, VfsInodeId,
    VfsResult,
};
use smallvec::SmallVec;
use std::collections::VecDeque;
use std::sync::Arc;

#[derive(Clone, Copy, Debug)]
pub struct WalkFlags {
    pub follow_symlinks: bool,
    pub follow_final_symlink: bool,
    pub must_be_dir: bool,
    pub allow_empty_path: bool,
    pub max_symlinks: u16,
    pub resolve_beneath: bool,
    pub in_root: bool,
}

impl WalkFlags {
    pub fn new(ctx: &VfsContext) -> Self {
        Self {
            follow_symlinks: true,
            follow_final_symlink: true,
            must_be_dir: false,
            allow_empty_path: false,
            max_symlinks: ctx.config.max_symlinks,
            resolve_beneath: false,
            in_root: false,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct TraversalInfo {
    pub components_walked: usize,
    pub symlinks_followed: u16,
}

#[derive(Clone)]
pub struct Resolved {
    pub mount: crate::MountId,
    pub inode: VfsInodeId,
    pub node: Arc<dyn FsNode>,
    pub parent: Option<Box<ResolvedParent>>,
    pub traversal: TraversalInfo,
}

#[derive(Clone)]
pub struct ResolvedParent {
    pub dir: Resolved,
    pub name: VfsNameBuf,
    pub had_trailing_slash: bool,
}

pub struct ResolutionRequest<'a> {
    pub ctx: &'a VfsContext,
    pub base: VfsBaseDir<'a>,
    pub path: &'a VfsPath,
    pub flags: WalkFlags,
}

pub struct PathWalker {
    mount_table: Arc<MountTable>,
}

impl PathWalker {
    pub fn new(mount_table: Arc<MountTable>) -> Self {
        Self { mount_table }
    }

    pub fn resolve(&self, req: ResolutionRequest<'_>) -> VfsResult<Resolved> {
        let ResolveOutcome::Final(resolved) = self.resolve_internal(req, ResolveMode::Final)?
        else {
            return Err(VfsError::new(VfsErrorKind::Internal, "path.resolve.final"));
        };
        Ok(resolved)
    }

    pub fn resolve_parent(&self, req: ResolutionRequest<'_>) -> VfsResult<ResolvedParent> {
        let ResolveOutcome::Parent(parent) = self.resolve_internal(req, ResolveMode::Parent)?
        else {
            return Err(VfsError::new(VfsErrorKind::Internal, "path.resolve_parent"));
        };
        Ok(parent)
    }

    pub fn resolve_at_component_boundary(&self, req: ResolutionRequest<'_>) -> VfsResult<Resolved> {
        self.resolve_internal(req, ResolveMode::Boundary)
            .map(|outcome| match outcome {
                ResolveOutcome::Final(resolved) => resolved,
                ResolveOutcome::Parent(parent) => parent.dir,
            })
    }

    fn resolve_internal(
        &self,
        req: ResolutionRequest<'_>,
        mode: ResolveMode,
    ) -> VfsResult<ResolveOutcome> {
        let inner = self.mount_table.snapshot();
        let (mut current, base_parent) = self.start_node(&inner, &req)?;
        let root_anchor = if req.flags.in_root {
            current.clone()
        } else {
            self.root_node(&inner)?
        };

        if req.path.is_empty() {
            if !req.flags.allow_empty_path {
                return Err(VfsError::new(
                    VfsErrorKind::InvalidInput,
                    "path.resolve.empty",
                ));
            }
            if matches!(mode, ResolveMode::Parent) {
                return Err(VfsError::new(
                    VfsErrorKind::InvalidInput,
                    "path.resolve.parent.empty",
                ));
            }
            let resolved = self.resolved_from_node(&current, TraversalInfo::default());
            if req.flags.must_be_dir && resolved.node.file_type() != VfsFileType::Directory {
                return Err(VfsError::new(VfsErrorKind::NotDir, "path.resolve.must_dir"));
            }
            return Ok(ResolveOutcome::Final(resolved));
        }

        if req.flags.resolve_beneath && req.path.is_absolute() {
            return Err(VfsError::new(
                VfsErrorKind::CrossDevice,
                "path.resolve.beneath.absolute",
            ));
        }
        let mut stack: SmallVec<[NodeRef; 8]> = SmallVec::new();
        if let Some(parent) = base_parent {
            stack.push(parent);
        }
        let min_stack_len = if req.flags.resolve_beneath {
            stack.len()
        } else {
            0
        };

        let mut queue = WorkQueue::from_path(req.path);
        let had_trailing_slash = req.path.has_trailing_slash();

        if req.path.is_absolute() {
            queue.pop_root();
        }
        if req.flags.in_root && req.path.is_absolute() {
            stack.clear();
        }

        if queue.is_empty() {
            if matches!(mode, ResolveMode::Parent) {
                return Err(VfsError::new(
                    VfsErrorKind::InvalidInput,
                    "path.resolve.parent.empty",
                ));
            }
            let resolved = self.resolved_from_node(&current, TraversalInfo::default());
            if req.flags.must_be_dir && resolved.node.file_type() != VfsFileType::Directory {
                return Err(VfsError::new(VfsErrorKind::NotDir, "path.resolve.must_dir"));
            }
            return Ok(ResolveOutcome::Final(resolved));
        }

        let mut traversal = TraversalInfo::default();
        let mut last_parent: Option<NodeRef> = None;
        let mut last_name: Option<VfsNameBuf> = None;

        while let Some(component) = queue.pop_front() {
            traversal.components_walked += 1;
            match component {
                WorkComponent::RootDir => {
                    if req.flags.resolve_beneath {
                        return Err(VfsError::new(
                            VfsErrorKind::CrossDevice,
                            "path.resolve.beneath.rootdir",
                        ));
                    }
                    current = root_anchor.clone();
                    stack.clear();
                }
                WorkComponent::CurDir => {
                    if queue.is_empty() && matches!(mode, ResolveMode::Parent) {
                        return Err(VfsError::new(
                            VfsErrorKind::InvalidInput,
                            "path.resolve.parent.curdir",
                        ));
                    }
                }
                WorkComponent::ParentDir => {
                    if queue.is_empty() && matches!(mode, ResolveMode::Parent) {
                        return Err(VfsError::new(
                            VfsErrorKind::InvalidInput,
                            "path.resolve.parent.parentdir",
                        ));
                    }
                    self.check_traverse_permission(req.ctx, &current)?;
                    if req.flags.resolve_beneath && stack.len() == min_stack_len {
                        return Err(VfsError::new(
                            VfsErrorKind::CrossDevice,
                            "path.resolve.beneath.parent",
                        ));
                    }
                    if let Some(prev) = stack.pop() {
                        current = prev;
                        continue;
                    }

                    if let Some(parent) = self.try_mount_parent(&inner, &current) {
                        current = parent;
                    }
                }
                WorkComponent::Normal(name) => {
                    let name_bytes = name;
                    let name_ref = self.validate_name(req.ctx, &name_bytes)?;
                    let is_final = queue.is_empty();
                    let name_buf =
                        if is_final {
                            Some(VfsNameBuf::new(name_bytes.clone()).map_err(|_| {
                                VfsError::new(VfsErrorKind::InvalidInput, "path.name")
                            })?)
                        } else {
                            None
                        };

                    if is_final && matches!(mode, ResolveMode::Parent) {
                        if current.node().file_type() != VfsFileType::Directory {
                            return Err(VfsError::new(
                                VfsErrorKind::NotDir,
                                "path.resolve.not_dir",
                            ));
                        }
                        self.check_traverse_permission(req.ctx, &current)?;
                        let parent_dir = self.resolved_from_node(&current, traversal.clone());
                        return Ok(ResolveOutcome::Parent(ResolvedParent {
                            dir: parent_dir,
                            name: name_buf.expect("final component must have name"),
                            had_trailing_slash,
                        }));
                    }

                    if current.node().file_type() != VfsFileType::Directory {
                        return Err(VfsError::new(VfsErrorKind::NotDir, "path.resolve.not_dir"));
                    }
                    self.check_traverse_permission(req.ctx, &current)?;

                    let child = current.node().lookup(&name_ref)?;
                    let child_ref = NodeRef::new(current.mount(), child);

                    if child_ref.node().file_type() == VfsFileType::Symlink {
                        let follow = if is_final {
                            req.flags.follow_final_symlink
                        } else {
                            req.flags.follow_symlinks
                        };

                        if follow {
                            traversal.symlinks_followed += 1;
                            if traversal.symlinks_followed > req.flags.max_symlinks {
                                return Err(VfsError::new(
                                    VfsErrorKind::TooManySymlinks,
                                    "path.resolve.symlink_depth",
                                ));
                            }

                            let target = child_ref.node().readlink()?;
                            queue.inject_symlink(target);
                            if queue.is_absolute_head() {
                                if req.flags.resolve_beneath {
                                    return Err(VfsError::new(
                                        VfsErrorKind::CrossDevice,
                                        "path.resolve.beneath.symlink_absolute",
                                    ));
                                }
                                current = root_anchor.clone();
                                stack.clear();
                            }
                            continue;
                        }

                        if !is_final {
                            return Err(VfsError::new(
                                VfsErrorKind::NotDir,
                                "path.resolve.symlink_no_follow",
                            ));
                        }
                        // Final symlink with NOFOLLOW: fall through as terminal entry.
                    }

                    if !is_final && child_ref.node().file_type() != VfsFileType::Directory {
                        return Err(VfsError::new(VfsErrorKind::NotDir, "path.resolve.not_dir"));
                    }

                    if is_final && matches!(mode, ResolveMode::Final) {
                        last_parent = Some(current.clone());
                        last_name = name_buf.clone();
                    }

                    let child_inode = make_vfs_inode(child_ref.mount(), child_ref.node().inode());
                    if let Some(mount_id) =
                        MountTable::enter_if_mountpoint(&inner, current.mount(), child_inode)
                    {
                        stack.push(child_ref.clone());
                        current = self.mount_root_node(&inner, mount_id)?;
                        continue;
                    }

                    stack.push(current.clone());
                    current = child_ref;
                }
            }
        }

        let mut resolved = self.resolved_from_node(&current, traversal.clone());
        if let (Some(parent), Some(name)) = (last_parent, last_name) {
            resolved.parent = Some(Box::new(ResolvedParent {
                dir: self.resolved_from_node(&parent, traversal),
                name,
                had_trailing_slash,
            }));
        }
        if (req.flags.must_be_dir || had_trailing_slash)
            && resolved.node.file_type() != VfsFileType::Directory
        {
            return Err(VfsError::new(VfsErrorKind::NotDir, "path.resolve.must_dir"));
        }

        Ok(ResolveOutcome::Final(resolved))
    }

    fn start_node(
        &self,
        inner: &MountTableInnerRef,
        req: &ResolutionRequest<'_>,
    ) -> VfsResult<(NodeRef, Option<NodeRef>)> {
        if req.path.is_absolute() && !req.flags.in_root {
            let root = self.root_node(inner)?;
            return Ok((root, None));
        }

        match req.base {
            VfsBaseDir::Cwd => {
                let inode = req.ctx.cwd.inode();
                Ok((
                    NodeRef::new(inode.mount, req.ctx.cwd.node().clone()),
                    req.ctx.cwd.parent(),
                ))
            }
            VfsBaseDir::Handle(dir) => {
                let inode = dir.inode();
                Ok((NodeRef::new(inode.mount, dir.node().clone()), dir.parent()))
            }
        }
    }

    fn root_node(&self, inner: &MountTableInnerRef) -> VfsResult<NodeRef> {
        let root_mount = inner.root;
        let (root_inode, fs) = MountTable::mount_root(inner, root_mount)
            .ok_or_else(|| VfsError::new(VfsErrorKind::Internal, "path.root"))?;
        let node = fs.root();
        Ok(NodeRef::new(root_inode.mount, node))
    }

    fn mount_root_node(
        &self,
        inner: &MountTableInnerRef,
        mount: crate::MountId,
    ) -> VfsResult<NodeRef> {
        let (root_inode, fs) = MountTable::mount_root(inner, mount)
            .ok_or_else(|| VfsError::new(VfsErrorKind::NotFound, "path.mount_root"))?;
        let node = fs.root();
        Ok(NodeRef::new(root_inode.mount, node))
    }

    fn try_mount_parent(&self, inner: &MountTableInnerRef, current: &NodeRef) -> Option<NodeRef> {
        let (root_inode, _) = MountTable::mount_root_any(inner, current.mount())?;
        if current.inode_id() != root_inode {
            return None;
        }
        let (parent_mount, mountpoint_inode) =
            MountTable::parent_of_mount_root(inner, current.mount())?;
        let (_, fs) = MountTable::mount_root_any(inner, parent_mount)?;
        let node = fs.node_by_inode(mountpoint_inode.backend)?;
        Some(NodeRef::new(parent_mount, node))
    }

    fn resolved_from_node(&self, node: &NodeRef, traversal: TraversalInfo) -> Resolved {
        Resolved {
            mount: node.mount(),
            inode: node.inode_id(),
            node: node.node().clone(),
            parent: None,
            traversal,
        }
    }

    fn validate_name<'a>(&self, ctx: &VfsContext, name: &'a [u8]) -> VfsResult<VfsName<'a>> {
        if name.len() > ctx.config.max_name_len {
            return Err(VfsError::new(VfsErrorKind::NameTooLong, "path.name"));
        }
        VfsName::new(name).map_err(|_| VfsError::new(VfsErrorKind::InvalidInput, "path.name"))
    }

    fn check_traverse_permission(&self, ctx: &VfsContext, current: &NodeRef) -> VfsResult<()> {
        let meta = current.node().metadata()?;
        ctx.policy.check_path_component_traverse(ctx, &meta)
    }
}

enum ResolveMode {
    Final,
    Parent,
    Boundary,
}

enum ResolveOutcome {
    Final(Resolved),
    Parent(ResolvedParent),
}

#[derive(Clone)]
pub struct ResolvedAsync {
    pub mount: crate::MountId,
    pub inode: VfsInodeId,
    pub node: Arc<dyn FsNodeAsync>,
    pub parent: Option<Box<ResolvedParentAsync>>,
    pub traversal: TraversalInfo,
}

#[derive(Clone)]
pub struct ResolvedParentAsync {
    pub dir: ResolvedAsync,
    pub name: VfsNameBuf,
    pub had_trailing_slash: bool,
}

pub struct ResolutionRequestAsync<'a> {
    pub ctx: &'a VfsContext,
    pub base: VfsBaseDirAsync<'a>,
    pub path: &'a VfsPath,
    pub flags: WalkFlags,
}

pub struct PathWalkerAsync {
    mount_table: Arc<MountTable>,
}

impl PathWalkerAsync {
    pub fn new(mount_table: Arc<MountTable>) -> Self {
        Self { mount_table }
    }

    pub async fn resolve(&self, req: ResolutionRequestAsync<'_>) -> VfsResult<ResolvedAsync> {
        let ResolveOutcomeAsync::Final(resolved) = self.resolve_internal(req, ResolveMode::Final).await?
        else {
            return Err(VfsError::new(VfsErrorKind::Internal, "path_async.resolve.final"));
        };
        Ok(resolved)
    }

    pub async fn resolve_parent(
        &self,
        req: ResolutionRequestAsync<'_>,
    ) -> VfsResult<ResolvedParentAsync> {
        let ResolveOutcomeAsync::Parent(parent) =
            self.resolve_internal(req, ResolveMode::Parent).await?
        else {
            return Err(VfsError::new(
                VfsErrorKind::Internal,
                "path_async.resolve_parent",
            ));
        };
        Ok(parent)
    }

    pub async fn resolve_at_component_boundary(
        &self,
        req: ResolutionRequestAsync<'_>,
    ) -> VfsResult<ResolvedAsync> {
        self.resolve_internal(req, ResolveMode::Boundary)
            .await
            .map(|outcome| match outcome {
                ResolveOutcomeAsync::Final(resolved) => resolved,
                ResolveOutcomeAsync::Parent(parent) => parent.dir,
            })
    }

    async fn resolve_internal(
        &self,
        req: ResolutionRequestAsync<'_>,
        mode: ResolveMode,
    ) -> VfsResult<ResolveOutcomeAsync> {
        let inner = self.mount_table.snapshot();
        let (mut current, base_parent) = self.start_node(&inner, &req).await?;
        let root_anchor = if req.flags.in_root {
            current.clone()
        } else {
            self.root_node(&inner).await?
        };

        if req.path.is_empty() {
            if !req.flags.allow_empty_path {
                return Err(VfsError::new(
                    VfsErrorKind::InvalidInput,
                    "path_async.resolve.empty",
                ));
            }
            if matches!(mode, ResolveMode::Parent) {
                return Err(VfsError::new(
                    VfsErrorKind::InvalidInput,
                    "path_async.resolve_parent.empty",
                ));
            }
            let resolved = self.resolved_from_node(&current, TraversalInfo::default());
            if req.flags.must_be_dir && resolved.node.file_type() != VfsFileType::Directory {
                return Err(VfsError::new(
                    VfsErrorKind::NotDir,
                    "path_async.resolve.must_dir",
                ));
            }
            return Ok(ResolveOutcomeAsync::Final(resolved));
        }

        if req.flags.resolve_beneath && req.path.is_absolute() {
            return Err(VfsError::new(
                VfsErrorKind::CrossDevice,
                "path_async.resolve.beneath.absolute",
            ));
        }
        let mut stack: SmallVec<[NodeRefAsync; 8]> = SmallVec::new();
        if let Some(parent) = base_parent {
            stack.push(parent);
        }
        let min_stack_len = if req.flags.resolve_beneath {
            stack.len()
        } else {
            0
        };

        let mut queue = WorkQueue::from_path(req.path);
        let had_trailing_slash = req.path.has_trailing_slash();

        if req.path.is_absolute() {
            queue.pop_root();
        }
        if req.flags.in_root && req.path.is_absolute() {
            stack.clear();
        }

        if queue.is_empty() {
            if matches!(mode, ResolveMode::Parent) {
                return Err(VfsError::new(
                    VfsErrorKind::InvalidInput,
                    "path_async.resolve_parent.empty",
                ));
            }
            let resolved = self.resolved_from_node(&current, TraversalInfo::default());
            if req.flags.must_be_dir && resolved.node.file_type() != VfsFileType::Directory {
                return Err(VfsError::new(
                    VfsErrorKind::NotDir,
                    "path_async.resolve.must_dir",
                ));
            }
            return Ok(ResolveOutcomeAsync::Final(resolved));
        }

        let mut traversal = TraversalInfo::default();
        let mut last_parent: Option<NodeRefAsync> = None;
        let mut last_name: Option<VfsNameBuf> = None;

        while let Some(component) = queue.pop_front() {
            traversal.components_walked += 1;
            match component {
                WorkComponent::RootDir => {
                    if req.flags.resolve_beneath {
                        return Err(VfsError::new(
                            VfsErrorKind::CrossDevice,
                            "path_async.resolve.beneath.rootdir",
                        ));
                    }
                    current = root_anchor.clone();
                    stack.clear();
                }
                WorkComponent::CurDir => {
                    if queue.is_empty() && matches!(mode, ResolveMode::Parent) {
                        return Err(VfsError::new(
                            VfsErrorKind::InvalidInput,
                            "path_async.resolve_parent.curdir",
                        ));
                    }
                }
                WorkComponent::ParentDir => {
                    if queue.is_empty() && matches!(mode, ResolveMode::Parent) {
                        return Err(VfsError::new(
                            VfsErrorKind::InvalidInput,
                            "path_async.resolve_parent.parentdir",
                        ));
                    }
                    self.check_traverse_permission(req.ctx, &current).await?;
                    if req.flags.resolve_beneath && stack.len() == min_stack_len {
                        return Err(VfsError::new(
                            VfsErrorKind::CrossDevice,
                            "path_async.resolve.beneath.parent",
                        ));
                    }
                    if let Some(prev) = stack.pop() {
                        current = prev;
                        continue;
                    }

                    if let Some(parent) = self.try_mount_parent(&inner, &current).await? {
                        current = parent;
                    }
                }
                WorkComponent::Normal(name) => {
                    let name_bytes = name;
                    let name_ref = self.validate_name(req.ctx, &name_bytes)?;
                    let is_final = queue.is_empty();
                    let name_buf =
                        if is_final {
                            Some(VfsNameBuf::new(name_bytes.clone()).map_err(|_| {
                                VfsError::new(VfsErrorKind::InvalidInput, "path_async.name")
                            })?)
                        } else {
                            None
                        };

                    if is_final && matches!(mode, ResolveMode::Parent) {
                        if current.node().file_type() != VfsFileType::Directory {
                            return Err(VfsError::new(
                                VfsErrorKind::NotDir,
                                "path_async.resolve.not_dir",
                            ));
                        }
                        self.check_traverse_permission(req.ctx, &current).await?;
                        let parent_dir = self.resolved_from_node(&current, traversal.clone());
                        return Ok(ResolveOutcomeAsync::Parent(ResolvedParentAsync {
                            dir: parent_dir,
                            name: name_buf.expect("final component must have name"),
                            had_trailing_slash,
                        }));
                    }

                    if current.node().file_type() != VfsFileType::Directory {
                        return Err(VfsError::new(
                            VfsErrorKind::NotDir,
                            "path_async.resolve.not_dir",
                        ));
                    }
                    self.check_traverse_permission(req.ctx, &current).await?;

                    let child = current.node().lookup(&name_ref).await?;
                    let child_ref = NodeRefAsync::new(current.mount(), child);

                    if child_ref.node().file_type() == VfsFileType::Symlink {
                        let follow = if is_final {
                            req.flags.follow_final_symlink
                        } else {
                            req.flags.follow_symlinks
                        };

                        if follow {
                            traversal.symlinks_followed += 1;
                            if traversal.symlinks_followed > req.flags.max_symlinks {
                                return Err(VfsError::new(
                                    VfsErrorKind::TooManySymlinks,
                                    "path_async.resolve.symlink_depth",
                                ));
                            }

                            let target = child_ref.node().readlink().await?;
                            queue.inject_symlink(target);
                            if queue.is_absolute_head() {
                                if req.flags.resolve_beneath {
                                    return Err(VfsError::new(
                                        VfsErrorKind::CrossDevice,
                                        "path_async.resolve.beneath.symlink_absolute",
                                    ));
                                }
                                current = root_anchor.clone();
                                stack.clear();
                            }
                            continue;
                        }

                        if !is_final {
                            return Err(VfsError::new(
                                VfsErrorKind::NotDir,
                                "path_async.resolve.symlink_no_follow",
                            ));
                        }
                        // Final symlink with NOFOLLOW: fall through as terminal entry.
                    }

                    if !is_final && child_ref.node().file_type() != VfsFileType::Directory {
                        return Err(VfsError::new(
                            VfsErrorKind::NotDir,
                            "path_async.resolve.not_dir",
                        ));
                    }

                    if is_final && matches!(mode, ResolveMode::Final) {
                        last_parent = Some(current.clone());
                        last_name = name_buf.clone();
                    }

                    let child_inode = make_vfs_inode(child_ref.mount(), child_ref.node().inode());
                    if let Some(mount_id) =
                        MountTable::enter_if_mountpoint(&inner, current.mount(), child_inode)
                    {
                        stack.push(child_ref.clone());
                        current = self.mount_root_node(&inner, mount_id).await?;
                        continue;
                    }

                    stack.push(current.clone());
                    current = child_ref;
                }
            }
        }

        let mut resolved = self.resolved_from_node(&current, traversal.clone());
        if let (Some(parent), Some(name)) = (last_parent, last_name) {
            resolved.parent = Some(Box::new(ResolvedParentAsync {
                dir: self.resolved_from_node(&parent, traversal),
                name,
                had_trailing_slash,
            }));
        }
        if (req.flags.must_be_dir || had_trailing_slash)
            && resolved.node.file_type() != VfsFileType::Directory
        {
            return Err(VfsError::new(
                VfsErrorKind::NotDir,
                "path_async.resolve.must_dir",
            ));
        }

        Ok(ResolveOutcomeAsync::Final(resolved))
    }

    async fn start_node(
        &self,
        inner: &MountTableInnerRef,
        req: &ResolutionRequestAsync<'_>,
    ) -> VfsResult<(NodeRefAsync, Option<NodeRefAsync>)> {
        if req.path.is_absolute() && !req.flags.in_root {
            let root = self.root_node(inner).await?;
            return Ok((root, None));
        }

        match req.base {
            VfsBaseDirAsync::Cwd => {
                let Some(cwd) = req.ctx.cwd_async.as_ref() else {
                    return Err(VfsError::new(
                        VfsErrorKind::NotSupported,
                        "path_async.resolve.cwd_missing",
                    ));
                };
                let inode = cwd.inode();
                Ok((
                    NodeRefAsync::new(inode.mount, cwd.node().clone()),
                    cwd.parent(),
                ))
            }
            VfsBaseDirAsync::Handle(dir) => {
                let inode = dir.inode();
                Ok((NodeRefAsync::new(inode.mount, dir.node().clone()), dir.parent()))
            }
        }
    }

    async fn root_node(&self, inner: &MountTableInnerRef) -> VfsResult<NodeRefAsync> {
        let root_mount = inner.root;
        let (root_inode, fs) = MountTable::mount_root_async(inner, root_mount)
            .ok_or_else(|| VfsError::new(VfsErrorKind::Internal, "path_async.root"))?;
        let node = fs.root().await?;
        Ok(NodeRefAsync::new(root_inode.mount, node))
    }

    async fn mount_root_node(
        &self,
        inner: &MountTableInnerRef,
        mount: crate::MountId,
    ) -> VfsResult<NodeRefAsync> {
        let (root_inode, fs) = MountTable::mount_root_async(inner, mount)
            .ok_or_else(|| VfsError::new(VfsErrorKind::NotFound, "path_async.mount_root"))?;
        let node = fs.root().await?;
        Ok(NodeRefAsync::new(root_inode.mount, node))
    }

    async fn try_mount_parent(
        &self,
        inner: &MountTableInnerRef,
        current: &NodeRefAsync,
    ) -> VfsResult<Option<NodeRefAsync>> {
        let (root_inode, _) = match MountTable::mount_root_any_async(inner, current.mount()) {
            Some(pair) => pair,
            None => return Ok(None),
        };
        if current.inode_id() != root_inode {
            return Ok(None);
        }
        let (parent_mount, mountpoint_inode) =
            match MountTable::parent_of_mount_root(inner, current.mount()) {
                Some(pair) => pair,
                None => return Ok(None),
            };
        let (_, fs) = match MountTable::mount_root_any_async(inner, parent_mount) {
            Some(pair) => pair,
            None => return Ok(None),
        };
        let node = match fs.node_by_inode(mountpoint_inode.backend).await? {
            Some(node) => node,
            None => return Ok(None),
        };
        Ok(Some(NodeRefAsync::new(parent_mount, node)))
    }

    fn resolved_from_node(&self, node: &NodeRefAsync, traversal: TraversalInfo) -> ResolvedAsync {
        ResolvedAsync {
            mount: node.mount(),
            inode: node.inode_id(),
            node: node.node().clone(),
            parent: None,
            traversal,
        }
    }

    fn validate_name<'a>(&self, ctx: &VfsContext, name: &'a [u8]) -> VfsResult<VfsName<'a>> {
        if name.len() > ctx.config.max_name_len {
            return Err(VfsError::new(VfsErrorKind::NameTooLong, "path_async.name"));
        }
        VfsName::new(name).map_err(|_| VfsError::new(VfsErrorKind::InvalidInput, "path_async.name"))
    }

    async fn check_traverse_permission(
        &self,
        ctx: &VfsContext,
        current: &NodeRefAsync,
    ) -> VfsResult<()> {
        let meta = current.node().metadata().await?;
        ctx.policy.check_path_component_traverse(ctx, &meta)
    }
}

enum ResolveOutcomeAsync {
    Final(ResolvedAsync),
    Parent(ResolvedParentAsync),
}

type MountTableInnerRef = crate::mount::MountTableInner;

#[derive(Clone)]
enum WorkComponent {
    RootDir,
    CurDir,
    ParentDir,
    Normal(Vec<u8>),
}

struct WorkQueue {
    items: VecDeque<WorkComponent>,
}

impl WorkQueue {
    fn from_path(path: &VfsPath) -> Self {
        let mut items = VecDeque::new();
        for comp in path.components() {
            match comp {
                VfsComponent::RootDir => items.push_back(WorkComponent::RootDir),
                VfsComponent::CurDir => items.push_back(WorkComponent::CurDir),
                VfsComponent::ParentDir => items.push_back(WorkComponent::ParentDir),
                VfsComponent::Normal(name) => items.push_back(WorkComponent::Normal(name.to_vec())),
            }
        }
        Self { items }
    }

    fn pop_root(&mut self) {
        if matches!(self.items.front(), Some(WorkComponent::RootDir)) {
            self.items.pop_front();
        }
    }

    fn is_absolute_head(&self) -> bool {
        matches!(self.items.front(), Some(WorkComponent::RootDir))
    }

    fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    fn pop_front(&mut self) -> Option<WorkComponent> {
        self.items.pop_front()
    }

    fn inject_symlink(&mut self, target: VfsPathBuf) {
        let mut next = VecDeque::new();
        for comp in target.as_path().components() {
            match comp {
                VfsComponent::RootDir => next.push_back(WorkComponent::RootDir),
                VfsComponent::CurDir => next.push_back(WorkComponent::CurDir),
                VfsComponent::ParentDir => next.push_back(WorkComponent::ParentDir),
                VfsComponent::Normal(name) => next.push_back(WorkComponent::Normal(name.to_vec())),
            }
        }
        while let Some(comp) = self.items.pop_front() {
            next.push_back(comp);
        }
        self.items = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::VfsCapabilities;
    use crate::flags::OpenOptions;
    use crate::node::{
        CreateFile, DirCursor, FsHandle, FsNode, MkdirOptions, ReadDirBatch, RenameOptions,
        SetMetadata, UnlinkOptions,
    };
    use crate::policy::AllowAllPolicy;
    use crate::provider::{AsyncFsFromSync, VfsRuntime};
    use crate::{
        BackendInodeId, Fs, MountId, VfsBaseDir, VfsConfig, VfsContext, VfsCred, VfsDirHandle,
        VfsErrorKind, VfsFileMode, VfsFileType, VfsHandleId, VfsInodeId, VfsMetadata, VfsPath,
        VfsTimespec,
    };
    use std::any::Any;
    use std::collections::HashMap;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};

    struct TestRuntime;

    impl VfsRuntime for TestRuntime {
        fn spawn_blocking_boxed(
            &self,
            f: Box<dyn FnOnce() -> Box<dyn Any + Send> + Send>,
        ) -> Pin<Box<dyn Future<Output = Box<dyn Any + Send>> + Send>> {
            Box::pin(async move {
                let handle = std::thread::spawn(f);
                match handle.join() {
                    Ok(value) => value,
                    Err(err) => std::panic::resume_unwind(err),
                }
            })
        }

        fn block_on_boxed<'a>(
            &'a self,
            fut: Pin<Box<dyn Future<Output = Box<dyn Any + Send>> + Send + 'a>>,
        ) -> Box<dyn Any + Send> {
            futures::executor::block_on(fut)
        }
    }

    #[derive(Debug)]
    struct TestNode {
        inode: BackendInodeId,
        file_type: VfsFileType,
        children: Mutex<HashMap<Vec<u8>, Arc<TestNode>>>,
        symlink_target: Option<VfsPathBuf>,
    }

    impl TestNode {
        fn dir(inode: BackendInodeId) -> Self {
            Self {
                inode,
                file_type: VfsFileType::Directory,
                children: Mutex::new(HashMap::new()),
                symlink_target: None,
            }
        }

        fn file(inode: BackendInodeId) -> Self {
            Self {
                inode,
                file_type: VfsFileType::RegularFile,
                children: Mutex::new(HashMap::new()),
                symlink_target: None,
            }
        }

        fn symlink(inode: BackendInodeId, target: VfsPathBuf) -> Self {
            Self {
                inode,
                file_type: VfsFileType::Symlink,
                children: Mutex::new(HashMap::new()),
                symlink_target: Some(target),
            }
        }

        fn unsupported<T>(&self, op: &'static str) -> VfsResult<T> {
            Err(VfsError::new(VfsErrorKind::NotSupported, op))
        }
    }

    impl FsNode for TestNode {
        fn inode(&self) -> BackendInodeId {
            self.inode
        }

        fn file_type(&self) -> VfsFileType {
            self.file_type
        }

        fn metadata(&self) -> VfsResult<VfsMetadata> {
            Ok(VfsMetadata {
                inode: VfsInodeId {
                    mount: MountId::from_index(0),
                    backend: self.inode,
                },
                file_type: self.file_type,
                mode: VfsFileMode(0),
                uid: 0,
                gid: 0,
                nlink: 1,
                size: 0,
                atime: VfsTimespec { secs: 0, nanos: 0 },
                mtime: VfsTimespec { secs: 0, nanos: 0 },
                ctime: VfsTimespec { secs: 0, nanos: 0 },
                rdev_major: 0,
                rdev_minor: 0,
            })
        }

        fn set_metadata(&self, _set: SetMetadata) -> VfsResult<()> {
            self.unsupported("test.set_metadata")
        }

        fn lookup(&self, name: &VfsName) -> VfsResult<Arc<dyn FsNode>> {
            if self.file_type != VfsFileType::Directory {
                return Err(VfsError::new(VfsErrorKind::NotDir, "test.lookup"));
            }
            let children = self.children.lock().unwrap();
            children
                .get(name.as_bytes())
                .cloned()
                .map(|node| node as Arc<dyn FsNode>)
                .ok_or_else(|| VfsError::new(VfsErrorKind::NotFound, "test.lookup"))
        }

        fn create_file(&self, _name: &VfsName, _opts: CreateFile) -> VfsResult<Arc<dyn FsNode>> {
            self.unsupported("test.create_file")
        }

        fn mkdir(&self, _name: &VfsName, _opts: MkdirOptions) -> VfsResult<Arc<dyn FsNode>> {
            self.unsupported("test.mkdir")
        }

        fn unlink(&self, _name: &VfsName, _opts: UnlinkOptions) -> VfsResult<()> {
            self.unsupported("test.unlink")
        }

        fn rmdir(&self, _name: &VfsName) -> VfsResult<()> {
            self.unsupported("test.rmdir")
        }

        fn read_dir(&self, _cursor: Option<DirCursor>, _max: usize) -> VfsResult<ReadDirBatch> {
            self.unsupported("test.read_dir")
        }

        fn rename(
            &self,
            _old_name: &VfsName,
            _new_parent: &dyn FsNode,
            _new_name: &VfsName,
            _opts: RenameOptions,
        ) -> VfsResult<()> {
            self.unsupported("test.rename")
        }

        fn open(&self, _opts: OpenOptions) -> VfsResult<Arc<dyn FsHandle>> {
            self.unsupported("test.open")
        }

        fn link(&self, _existing: &dyn FsNode, _new_name: &VfsName) -> VfsResult<()> {
            self.unsupported("test.link")
        }

        fn symlink(&self, _new_name: &VfsName, _target: &VfsPath) -> VfsResult<()> {
            self.unsupported("test.symlink")
        }

        fn readlink(&self) -> VfsResult<VfsPathBuf> {
            self.symlink_target
                .clone()
                .ok_or_else(|| VfsError::new(VfsErrorKind::NotSupported, "test.readlink"))
        }
    }

    struct TestFs {
        root: Arc<TestNode>,
        nodes: Mutex<HashMap<BackendInodeId, Arc<TestNode>>>,
        next_inode: AtomicU64,
    }

    impl TestFs {
        fn new() -> Arc<Self> {
            let root = Arc::new(TestNode::dir(
                BackendInodeId::new(1).expect("non-zero inode"),
            ));
            let mut nodes = HashMap::new();
            nodes.insert(root.inode, root.clone());
            Arc::new(Self {
                root,
                nodes: Mutex::new(nodes),
                next_inode: AtomicU64::new(2),
            })
        }

        fn alloc_inode(&self) -> BackendInodeId {
            let raw = self.next_inode.fetch_add(1, Ordering::SeqCst);
            BackendInodeId::new(raw).expect("non-zero inode")
        }

        fn register(&self, node: Arc<TestNode>) {
            self.nodes.lock().unwrap().insert(node.inode, node);
        }

        fn add_dir(&self, parent: &Arc<TestNode>, name: &[u8]) -> Arc<TestNode> {
            let node = Arc::new(TestNode::dir(self.alloc_inode()));
            parent
                .children
                .lock()
                .unwrap()
                .insert(name.to_vec(), node.clone());
            self.register(node.clone());
            node
        }

        fn add_file(&self, parent: &Arc<TestNode>, name: &[u8]) -> Arc<TestNode> {
            let node = Arc::new(TestNode::file(self.alloc_inode()));
            parent
                .children
                .lock()
                .unwrap()
                .insert(name.to_vec(), node.clone());
            self.register(node.clone());
            node
        }

        fn add_symlink(&self, parent: &Arc<TestNode>, name: &[u8], target: &[u8]) -> Arc<TestNode> {
            let node = Arc::new(TestNode::symlink(
                self.alloc_inode(),
                VfsPathBuf::from_bytes(target.to_vec()),
            ));
            parent
                .children
                .lock()
                .unwrap()
                .insert(name.to_vec(), node.clone());
            self.register(node.clone());
            node
        }
    }

    impl Fs for TestFs {
        fn provider_name(&self) -> &'static str {
            "test"
        }

        fn capabilities(&self) -> VfsCapabilities {
            VfsCapabilities::NONE
        }

        fn root(&self) -> Arc<dyn FsNode> {
            self.root.clone()
        }

        fn node_by_inode(&self, inode: BackendInodeId) -> Option<Arc<dyn FsNode>> {
            self.nodes
                .lock()
                .unwrap()
                .get(&inode)
                .cloned()
                .map(|node| node as Arc<dyn FsNode>)
        }
    }

    fn make_dir_handle(
        mount_table: &MountTable,
        mount: MountId,
        node: Arc<dyn FsNode>,
        parent: Option<NodeRef>,
        id: u64,
    ) -> VfsDirHandle {
        let guard = mount_table.guard(mount).expect("mount guard");
        VfsDirHandle::new(
            VfsHandleId(id),
            guard,
            make_vfs_inode(mount, node.inode()),
            node,
            parent,
        )
    }

    fn make_ctx(cwd: VfsDirHandle) -> VfsContext {
        VfsContext::new(
            VfsCred::root(),
            cwd,
            Arc::new(VfsConfig::default()),
            Arc::new(AllowAllPolicy),
        )
    }

    fn mount_table_for(fs: &Arc<TestFs>) -> Arc<MountTable> {
        let fs_arc: Arc<dyn Fs> = fs.clone();
        let runtime: Arc<dyn VfsRuntime> = Arc::new(TestRuntime);
        let fs_async: Arc<dyn crate::FsAsync> =
            Arc::new(AsyncFsFromSync::new(fs_arc.clone(), runtime));
        Arc::new(MountTable::new(fs_arc, fs_async).expect("mount table"))
    }

    fn mount_secondary(
        mount_table: &MountTable,
        mountpoint_inode: VfsInodeId,
        secondary: &Arc<TestFs>,
    ) -> MountId {
        let secondary_fs: Arc<dyn Fs> = secondary.clone();
        let runtime: Arc<dyn VfsRuntime> = Arc::new(TestRuntime);
        let secondary_async: Arc<dyn crate::FsAsync> =
            Arc::new(AsyncFsFromSync::new(secondary_fs.clone(), runtime));
        mount_table
            .mount(
                MountId::from_index(0),
                mountpoint_inode,
                secondary_fs,
                secondary_async,
                secondary.root.inode(),
                crate::provider::MountFlags::empty(),
            )
            .expect("mount secondary fs")
    }

    #[test]
    fn symlink_follow_and_nofollow() {
        let fs = TestFs::new();
        let root = fs.root.clone();
        let dir = fs.add_dir(&root, b"dir");
        fs.add_file(&dir, b"child");
        fs.add_symlink(&root, b"linkdir", b"dir");
        fs.add_symlink(&root, b"final", b"dir/child");

        let mount_table = mount_table_for(&fs);
        let cwd = make_dir_handle(&mount_table, MountId::from_index(0), root.clone(), None, 1);
        let ctx = make_ctx(cwd);
        let walker = PathWalker::new(mount_table.clone());

        let mut flags = WalkFlags::new(&ctx);
        flags.follow_symlinks = false;
        let err = match walker.resolve(ResolutionRequest {
            ctx: &ctx,
            base: VfsBaseDir::Cwd,
            path: VfsPath::new(b"linkdir/child"),
            flags,
        }) {
            Ok(_) => panic!("intermediate symlink without follow"),
            Err(err) => err,
        };
        assert_eq!(err.kind(), VfsErrorKind::NotDir);

        let mut flags = WalkFlags::new(&ctx);
        flags.follow_symlinks = true;
        let resolved = walker
            .resolve(ResolutionRequest {
                ctx: &ctx,
                base: VfsBaseDir::Cwd,
                path: VfsPath::new(b"linkdir/child"),
                flags,
            })
            .expect("symlink follow should resolve");
        assert_eq!(resolved.node.file_type(), VfsFileType::RegularFile);

        let mut flags = WalkFlags::new(&ctx);
        flags.follow_final_symlink = false;
        let resolved = walker
            .resolve(ResolutionRequest {
                ctx: &ctx,
                base: VfsBaseDir::Cwd,
                path: VfsPath::new(b"final"),
                flags,
            })
            .expect("final symlink nofollow resolves");
        assert_eq!(resolved.node.file_type(), VfsFileType::Symlink);
    }

    #[test]
    fn symlink_depth_limit() {
        let fs = TestFs::new();
        let root = fs.root.clone();
        fs.add_file(&root, b"target");
        fs.add_symlink(&root, b"a", b"b");
        fs.add_symlink(&root, b"b", b"c");
        fs.add_symlink(&root, b"c", b"target");

        let mount_table = mount_table_for(&fs);
        let cwd = make_dir_handle(&mount_table, MountId::from_index(0), root.clone(), None, 1);
        let ctx = make_ctx(cwd);
        let walker = PathWalker::new(mount_table.clone());

        let mut flags = WalkFlags::new(&ctx);
        flags.max_symlinks = 2;
        let err = match walker.resolve(ResolutionRequest {
            ctx: &ctx,
            base: VfsBaseDir::Cwd,
            path: VfsPath::new(b"a"),
            flags,
        }) {
            Ok(_) => panic!("symlink depth should be enforced"),
            Err(err) => err,
        };
        assert_eq!(err.kind(), VfsErrorKind::TooManySymlinks);
    }

    #[test]
    fn trailing_slash_rules() {
        let fs = TestFs::new();
        let root = fs.root.clone();
        fs.add_file(&root, b"file");
        fs.add_dir(&root, b"dir");

        let mount_table = mount_table_for(&fs);
        let cwd = make_dir_handle(&mount_table, MountId::from_index(0), root.clone(), None, 1);
        let ctx = make_ctx(cwd);
        let walker = PathWalker::new(mount_table.clone());

        let flags = WalkFlags::new(&ctx);
        let err = match walker.resolve(ResolutionRequest {
            ctx: &ctx,
            base: VfsBaseDir::Cwd,
            path: VfsPath::new(b"file/"),
            flags,
        }) {
            Ok(_) => panic!("file/ should be NotDir"),
            Err(err) => err,
        };
        assert_eq!(err.kind(), VfsErrorKind::NotDir);

        let flags = WalkFlags::new(&ctx);
        let resolved = walker
            .resolve(ResolutionRequest {
                ctx: &ctx,
                base: VfsBaseDir::Cwd,
                path: VfsPath::new(b"dir/"),
                flags,
            })
            .expect("dir/ should resolve");
        assert_eq!(resolved.node.file_type(), VfsFileType::Directory);
    }

    #[test]
    fn allow_empty_path_behavior() {
        let fs = TestFs::new();
        let root = fs.root.clone();
        let mount_table = mount_table_for(&fs);
        let cwd = make_dir_handle(&mount_table, MountId::from_index(0), root.clone(), None, 1);
        let ctx = make_ctx(cwd);
        let walker = PathWalker::new(mount_table.clone());

        let flags = WalkFlags::new(&ctx);
        let err = match walker.resolve(ResolutionRequest {
            ctx: &ctx,
            base: VfsBaseDir::Cwd,
            path: VfsPath::new(b""),
            flags,
        }) {
            Ok(_) => panic!("empty path should fail by default"),
            Err(err) => err,
        };
        assert_eq!(err.kind(), VfsErrorKind::InvalidInput);

        let mut flags = WalkFlags::new(&ctx);
        flags.allow_empty_path = true;
        let resolved = walker
            .resolve(ResolutionRequest {
                ctx: &ctx,
                base: VfsBaseDir::Cwd,
                path: VfsPath::new(b""),
                flags,
            })
            .expect("empty path should resolve with allow");
        assert_eq!(
            resolved.inode,
            make_vfs_inode(MountId::from_index(0), root.inode())
        );
    }

    #[test]
    fn in_root_scopes_absolute_paths_and_symlinks() {
        let fs = TestFs::new();
        let root = fs.root.clone();
        let sandbox = fs.add_dir(&root, b"sandbox");
        let etc = fs.add_dir(&sandbox, b"etc");
        fs.add_symlink(&sandbox, b"abs", b"/etc");

        let mount_table = mount_table_for(&fs);
        let parent_ref = NodeRef::new(MountId::from_index(0), root.clone());
        let base_handle = make_dir_handle(
            &mount_table,
            MountId::from_index(0),
            sandbox.clone(),
            Some(parent_ref),
            2,
        );
        let cwd = make_dir_handle(&mount_table, MountId::from_index(0), root.clone(), None, 1);
        let ctx = make_ctx(cwd);
        let walker = PathWalker::new(mount_table.clone());

        let mut flags = WalkFlags::new(&ctx);
        flags.in_root = true;
        let resolved = walker
            .resolve(ResolutionRequest {
                ctx: &ctx,
                base: VfsBaseDir::Handle(&base_handle),
                path: VfsPath::new(b"/etc"),
                flags,
            })
            .expect("absolute path should resolve under base");
        assert_eq!(
            resolved.inode,
            make_vfs_inode(MountId::from_index(0), etc.inode())
        );

        let mut flags = WalkFlags::new(&ctx);
        flags.in_root = true;
        let resolved = walker
            .resolve(ResolutionRequest {
                ctx: &ctx,
                base: VfsBaseDir::Handle(&base_handle),
                path: VfsPath::new(b"abs"),
                flags,
            })
            .expect("absolute symlink should resolve under base");
        assert_eq!(
            resolved.inode,
            make_vfs_inode(MountId::from_index(0), etc.inode())
        );
    }

    #[test]
    fn resolve_beneath_blocks_escape() {
        let fs = TestFs::new();
        let root = fs.root.clone();
        let sandbox = fs.add_dir(&root, b"sandbox");
        let sub = fs.add_dir(&sandbox, b"sub");
        fs.add_dir(&sandbox, b"etc");
        fs.add_symlink(&sub, b"abs", b"/etc");

        let mount_table = mount_table_for(&fs);
        let parent_ref = NodeRef::new(MountId::from_index(0), sandbox.clone());
        let base_handle = make_dir_handle(
            &mount_table,
            MountId::from_index(0),
            sub.clone(),
            Some(parent_ref),
            2,
        );
        let cwd = make_dir_handle(&mount_table, MountId::from_index(0), root.clone(), None, 1);
        let ctx = make_ctx(cwd);
        let walker = PathWalker::new(mount_table.clone());

        let mut flags = WalkFlags::new(&ctx);
        flags.resolve_beneath = true;
        let err = match walker.resolve(ResolutionRequest {
            ctx: &ctx,
            base: VfsBaseDir::Handle(&base_handle),
            path: VfsPath::new(b"../x"),
            flags,
        }) {
            Ok(_) => panic!(".. should be blocked under resolve_beneath"),
            Err(err) => err,
        };
        assert_eq!(err.kind(), VfsErrorKind::CrossDevice);

        let mut flags = WalkFlags::new(&ctx);
        flags.resolve_beneath = true;
        let err = match walker.resolve(ResolutionRequest {
            ctx: &ctx,
            base: VfsBaseDir::Handle(&base_handle),
            path: VfsPath::new(b"/x"),
            flags,
        }) {
            Ok(_) => panic!("absolute path should be blocked"),
            Err(err) => err,
        };
        assert_eq!(err.kind(), VfsErrorKind::CrossDevice);

        let mut flags = WalkFlags::new(&ctx);
        flags.resolve_beneath = true;
        let err = match walker.resolve(ResolutionRequest {
            ctx: &ctx,
            base: VfsBaseDir::Handle(&base_handle),
            path: VfsPath::new(b"abs"),
            flags,
        }) {
            Ok(_) => panic!("absolute symlink target should be blocked"),
            Err(err) => err,
        };
        assert_eq!(err.kind(), VfsErrorKind::CrossDevice);
    }

    #[test]
    fn mount_parent_resolution_on_dotdot() {
        let fs = TestFs::new();
        let root = fs.root.clone();
        let mnt = fs.add_dir(&root, b"mnt");

        let mount_table = mount_table_for(&fs);
        let secondary = TestFs::new();
        let secondary_root = secondary.root.clone();
        let secondary_fs: Arc<dyn Fs> = secondary.clone();
        let runtime: Arc<dyn VfsRuntime> = Arc::new(TestRuntime);
        let secondary_async: Arc<dyn crate::FsAsync> =
            Arc::new(AsyncFsFromSync::new(secondary_fs.clone(), runtime));
        let mount_id = mount_table
            .mount(
                MountId::from_index(0),
                make_vfs_inode(MountId::from_index(0), mnt.inode()),
                secondary_fs,
                secondary_async,
                secondary_root.inode(),
                crate::provider::MountFlags::empty(),
            )
            .expect("mount secondary fs");

        let parent_ref = NodeRef::new(MountId::from_index(0), mnt.clone());
        let base_handle = make_dir_handle(
            &mount_table,
            mount_id,
            secondary_root.clone(),
            Some(parent_ref),
            2,
        );
        let cwd = make_dir_handle(&mount_table, MountId::from_index(0), root.clone(), None, 1);
        let ctx = make_ctx(cwd);
        let walker = PathWalker::new(mount_table.clone());

        let flags = WalkFlags::new(&ctx);
        let resolved = walker
            .resolve(ResolutionRequest {
                ctx: &ctx,
                base: VfsBaseDir::Handle(&base_handle),
                path: VfsPath::new(b".."),
                flags,
            })
            .expect(".. from mount root should return mountpoint");
        assert_eq!(
            resolved.inode,
            make_vfs_inode(MountId::from_index(0), mnt.inode())
        );
    }

    #[test]
    fn mount_enter_and_traverse_into_mounted_fs() {
        let fs = TestFs::new();
        let root = fs.root.clone();
        let mnt = fs.add_dir(&root, b"mnt");

        let secondary = TestFs::new();
        let secondary_root = secondary.root.clone();
        let secondary_sub = secondary.add_dir(&secondary_root, b"sub");

        let mount_table = mount_table_for(&fs);
        let mount_id = mount_secondary(
            &mount_table,
            make_vfs_inode(MountId::from_index(0), mnt.inode()),
            &secondary,
        );

        let cwd = make_dir_handle(&mount_table, MountId::from_index(0), root.clone(), None, 1);
        let ctx = make_ctx(cwd);
        let walker = PathWalker::new(mount_table.clone());

        let resolved = walker
            .resolve(ResolutionRequest {
                ctx: &ctx,
                base: VfsBaseDir::Cwd,
                path: VfsPath::new(b"/mnt/sub"),
                flags: WalkFlags::new(&ctx),
            })
            .expect("mount traversal should resolve mounted sub");
        assert_eq!(
            resolved.inode,
            make_vfs_inode(mount_id, secondary_sub.inode())
        );
    }

    #[test]
    fn mount_dotdot_inside_mounted_fs() {
        let fs = TestFs::new();
        let root = fs.root.clone();
        let mnt = fs.add_dir(&root, b"mnt");

        let secondary = TestFs::new();
        let secondary_root = secondary.root.clone();
        let dir = secondary.add_dir(&secondary_root, b"d");
        let _sub = secondary.add_dir(&dir, b"e");

        let mount_table = mount_table_for(&fs);
        let mount_id = mount_secondary(
            &mount_table,
            make_vfs_inode(MountId::from_index(0), mnt.inode()),
            &secondary,
        );

        let cwd = make_dir_handle(&mount_table, MountId::from_index(0), root.clone(), None, 1);
        let ctx = make_ctx(cwd);
        let walker = PathWalker::new(mount_table.clone());

        let resolved = walker
            .resolve(ResolutionRequest {
                ctx: &ctx,
                base: VfsBaseDir::Cwd,
                path: VfsPath::new(b"/mnt/d/e/.."),
                flags: WalkFlags::new(&ctx),
            })
            .expect("dotdot inside mount should resolve");
        assert_eq!(resolved.inode, make_vfs_inode(mount_id, dir.inode()));
        assert_eq!(resolved.node.file_type(), VfsFileType::Directory);
    }

    #[test]
    fn symlink_across_mount_boundary() {
        let fs = TestFs::new();
        let root = fs.root.clone();
        let mnt = fs.add_dir(&root, b"mnt");

        let secondary = TestFs::new();
        let secondary_root = secondary.root.clone();
        let secondary_sub = secondary.add_dir(&secondary_root, b"sub");

        let mount_table = mount_table_for(&fs);
        let mount_id = mount_secondary(
            &mount_table,
            make_vfs_inode(MountId::from_index(0), mnt.inode()),
            &secondary,
        );

        fs.add_symlink(&root, b"link", b"/mnt/sub");

        let cwd = make_dir_handle(&mount_table, MountId::from_index(0), root.clone(), None, 1);
        let ctx = make_ctx(cwd);
        let walker = PathWalker::new(mount_table.clone());

        let resolved = walker
            .resolve(ResolutionRequest {
                ctx: &ctx,
                base: VfsBaseDir::Cwd,
                path: VfsPath::new(b"/link"),
                flags: WalkFlags::new(&ctx),
            })
            .expect("symlink across mount should resolve");
        assert_eq!(
            resolved.inode,
            make_vfs_inode(mount_id, secondary_sub.inode())
        );
    }

    #[test]
    fn trailing_slash_uses_mounted_root() {
        let fs = TestFs::new();
        let root = fs.root.clone();
        let mnt = fs.add_dir(&root, b"mnt");

        let secondary = TestFs::new();
        let secondary_root = secondary.root.clone();

        let mount_table = mount_table_for(&fs);
        let mount_id = mount_secondary(
            &mount_table,
            make_vfs_inode(MountId::from_index(0), mnt.inode()),
            &secondary,
        );

        let cwd = make_dir_handle(&mount_table, MountId::from_index(0), root.clone(), None, 1);
        let ctx = make_ctx(cwd);
        let walker = PathWalker::new(mount_table.clone());

        let resolved = walker
            .resolve(ResolutionRequest {
                ctx: &ctx,
                base: VfsBaseDir::Cwd,
                path: VfsPath::new(b"/mnt/"),
                flags: WalkFlags::new(&ctx),
            })
            .expect("mount root with trailing slash should resolve");
        assert_eq!(
            resolved.inode,
            make_vfs_inode(mount_id, secondary_root.inode())
        );
        assert_eq!(resolved.node.file_type(), VfsFileType::Directory);
    }

    #[test]
    fn detached_mount_still_allows_handle_traversal() {
        let fs = TestFs::new();
        let root = fs.root.clone();
        let mnt = fs.add_dir(&root, b"mnt");
        let parent_sub = fs.add_dir(&mnt, b"sub");

        let secondary = TestFs::new();
        let secondary_root = secondary.root.clone();
        let secondary_sub = secondary.add_dir(&secondary_root, b"sub");

        let mount_table = mount_table_for(&fs);
        let mount_id = mount_secondary(
            &mount_table,
            make_vfs_inode(MountId::from_index(0), mnt.inode()),
            &secondary,
        );

        let base_handle = make_dir_handle(&mount_table, mount_id, secondary_root.clone(), None, 2);
        mount_table
            .unmount(mount_id, crate::mount::UnmountFlags::Detach)
            .expect("detach mount");

        let cwd = make_dir_handle(&mount_table, MountId::from_index(0), root.clone(), None, 1);
        let ctx = make_ctx(cwd);
        let walker = PathWalker::new(mount_table.clone());

        let resolved_parent = walker
            .resolve(ResolutionRequest {
                ctx: &ctx,
                base: VfsBaseDir::Cwd,
                path: VfsPath::new(b"/mnt/sub"),
                flags: WalkFlags::new(&ctx),
            })
            .expect("detached mount should resolve parent fs path");
        assert_eq!(
            resolved_parent.inode,
            make_vfs_inode(MountId::from_index(0), parent_sub.inode())
        );

        let resolved_detached = walker
            .resolve(ResolutionRequest {
                ctx: &ctx,
                base: VfsBaseDir::Handle(&base_handle),
                path: VfsPath::new(b"sub"),
                flags: WalkFlags::new(&ctx),
            })
            .expect("detached mount handle should still resolve");
        assert_eq!(
            resolved_detached.inode,
            make_vfs_inode(mount_id, secondary_sub.inode())
        );
    }
}
