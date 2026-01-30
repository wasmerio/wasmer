//! Policy hooks (allow/deny, confinement, permissions).

use crate::{
    OpenFlags, VfsAccess, VfsContext, VfsError, VfsErrorKind, VfsMetadata, VfsResult,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VfsMutationOp {
    CreateFile,
    CreateDir,
    Remove { is_dir: bool },
    Rename,
    Link,
    Symlink,
    SetMetadata,
}

pub trait VfsPolicy: Send + Sync + 'static {
    fn check_path_component_traverse(
        &self,
        _ctx: &VfsContext,
        _dir_meta: &VfsMetadata,
    ) -> VfsResult<()> {
        Ok(())
    }

    fn check_open(
        &self,
        _ctx: &VfsContext,
        _node_meta: &VfsMetadata,
        _open_flags: OpenFlags,
    ) -> VfsResult<()> {
        Ok(())
    }

    fn check_mutation(
        &self,
        _ctx: &VfsContext,
        _parent_dir_meta: &VfsMetadata,
        _op: VfsMutationOp,
    ) -> VfsResult<()> {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct AllowAllPolicy;

impl VfsPolicy for AllowAllPolicy {}

#[derive(Clone, Copy, Debug)]
pub struct PosixPolicy {
    pub enforce: bool,
    pub root_bypass: bool,
}

impl PosixPolicy {
    pub fn new(enforce: bool, root_bypass: bool) -> Self {
        Self {
            enforce,
            root_bypass,
        }
    }

    fn check_access(&self, ctx: &VfsContext, meta: &VfsMetadata, access: VfsAccess) -> VfsResult<()> {
        if !self.enforce || access.is_empty() {
            return Ok(());
        }

        if self.root_bypass && ctx.cred.uid == 0 {
            return Ok(());
        }

        let mask = access_mask(access);
        let mode = if ctx.cred.uid == meta.uid {
            meta.mode.owner_bits()
        } else if ctx.cred.gid == meta.gid || ctx.cred.groups.contains(&meta.gid) {
            meta.mode.group_bits()
        } else {
            meta.mode.other_bits()
        };

        if (mode & mask) == mask {
            Ok(())
        } else {
            Err(VfsError::new(
                VfsErrorKind::PermissionDenied,
                "policy.posix",
            ))
        }
    }
}

impl VfsPolicy for PosixPolicy {
    fn check_path_component_traverse(
        &self,
        ctx: &VfsContext,
        dir_meta: &VfsMetadata,
    ) -> VfsResult<()> {
        self.check_access(ctx, dir_meta, VfsAccess::EXEC)
    }

    fn check_open(
        &self,
        ctx: &VfsContext,
        node_meta: &VfsMetadata,
        open_flags: OpenFlags,
    ) -> VfsResult<()> {
        let mut access = VfsAccess::empty();
        if open_flags.contains(OpenFlags::READ) {
            access |= VfsAccess::READ;
        }
        if open_flags.contains(OpenFlags::WRITE) {
            access |= VfsAccess::WRITE;
        }
        self.check_access(ctx, node_meta, access)
    }

    fn check_mutation(
        &self,
        ctx: &VfsContext,
        parent_dir_meta: &VfsMetadata,
        _op: VfsMutationOp,
    ) -> VfsResult<()> {
        self.check_access(ctx, parent_dir_meta, VfsAccess::WRITE | VfsAccess::EXEC)
    }
}

fn access_mask(access: VfsAccess) -> u32 {
    let mut mask = 0;
    if access.contains(VfsAccess::READ) {
        mask |= 0o4;
    }
    if access.contains(VfsAccess::WRITE) {
        mask |= 0o2;
    }
    if access.contains(VfsAccess::EXEC) {
        mask |= 0o1;
    }
    mask
}
