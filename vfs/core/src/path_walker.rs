//! Mount-aware path traversal and resolution.

use crate::inode::{make_vfs_inode, NodeRef};
use crate::mount::MountTable;
use crate::node::FsNode;
use crate::path_types::{VfsComponent, VfsName, VfsNameBuf, VfsPath, VfsPathBuf};
use crate::{
    VfsBaseDir, VfsContext, VfsError, VfsErrorKind, VfsFileType, VfsInodeId, VfsResult,
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
    pub parent: Option<ResolvedParent>,
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
        self.resolve_internal(req, ResolveMode::Final)
    }

    pub fn resolve_parent(&self, req: ResolutionRequest<'_>) -> VfsResult<ResolvedParent> {
        let ResolveOutcome::Parent(parent) = self.resolve_internal(req, ResolveMode::Parent)? else {
            return Err(VfsError::new(VfsErrorKind::Internal, "path.resolve_parent"));
        };
        Ok(parent)
    }

    pub fn resolve_at_component_boundary(&self, req: ResolutionRequest<'_>) -> VfsResult<Resolved> {
        self.resolve_internal(req, ResolveMode::Boundary).map(|outcome| match outcome {
            ResolveOutcome::Final(resolved) => resolved,
            ResolveOutcome::Parent(parent) => parent.dir,
        })
    }

    fn resolve_internal(&self, req: ResolutionRequest<'_>, mode: ResolveMode) -> VfsResult<ResolveOutcome> {
        if req.flags.resolve_beneath || req.flags.in_root {
            return Err(VfsError::new(
                VfsErrorKind::NotSupported,
                "path.resolve.flags",
            ));
        }

        if req.path.is_empty() && !req.flags.allow_empty_path {
            return Err(VfsError::new(
                VfsErrorKind::InvalidInput,
                "path.resolve.empty",
            ));
        }

        let inner = self.mount_table.snapshot();
        let (mut current, base_parent) = self.start_node(&inner, &req)?;
        let mut stack: SmallVec<[NodeRef; 8]> = SmallVec::new();
        if let Some(parent) = base_parent {
            stack.push(parent);
        }

        let mut queue = WorkQueue::from_path(req.path);
        let had_trailing_slash = req.path.has_trailing_slash();

        if req.path.is_absolute() {
            queue.pop_root();
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

        while let Some(component) = queue.pop_front() {
            traversal.components_walked += 1;
            match component {
                WorkComponent::RootDir => {
                    current = self.root_node(&inner)?;
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

                    if is_final && matches!(mode, ResolveMode::Parent) {
                        let parent_dir = self.resolved_from_node(&current, traversal.clone());
                        let name_buf = VfsNameBuf::new(name_bytes).map_err(|_| {
                            VfsError::new(VfsErrorKind::InvalidInput, "path.name")
                        })?;
                        return Ok(ResolveOutcome::Parent(ResolvedParent {
                            dir: parent_dir,
                            name: name_buf,
                            had_trailing_slash,
                        }));
                    }

                    if current.node().file_type() != VfsFileType::Directory {
                        return Err(VfsError::new(VfsErrorKind::NotDir, "path.resolve.not_dir"));
                    }

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
                                current = self.root_node(&inner)?;
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

        let resolved = self.resolved_from_node(&current, traversal);
        if (req.flags.must_be_dir || had_trailing_slash)
            && resolved.node.file_type() != VfsFileType::Directory
        {
            return Err(VfsError::new(VfsErrorKind::NotDir, "path.resolve.must_dir"));
        }

        Ok(ResolveOutcome::Final(resolved))
    }

    fn start_node(
        &self,
        _inner: &MountTableInnerRef,
        req: &ResolutionRequest<'_>,
    ) -> VfsResult<(NodeRef, Option<NodeRef>)> {
        if req.path.is_absolute() {
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

    fn mount_root_node(&self, inner: &MountTableInnerRef, mount: crate::MountId) -> VfsResult<NodeRef> {
        let (root_inode, fs) = MountTable::mount_root(inner, mount)
            .ok_or_else(|| VfsError::new(VfsErrorKind::NotFound, "path.mount_root"))?;
        let node = fs.root();
        Ok(NodeRef::new(root_inode.mount, node))
    }

    fn try_mount_parent(&self, inner: &MountTableInnerRef, current: &NodeRef) -> Option<NodeRef> {
        let (root_inode, _) = MountTable::mount_root(inner, current.mount())?;
        if current.inode_id() != root_inode {
            return None;
        }
        let (parent_mount, mountpoint_inode) = MountTable::parent_of_mount_root(inner, current.mount())?;
        let (_, fs) = MountTable::mount_root(inner, parent_mount)?;
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
