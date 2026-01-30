//! Policy hooks (allow/deny, confinement, permissions).
//!
//! Phase 2.5 will implement POSIX permissions and confinement semantics using this interface.

use crate::{OpenOptions, VfsContext, VfsInodeId, VfsMetadata, VfsResult};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VfsOp {
    Read,
    Write,
    Exec,
    Create,
    Delete,
    Metadata,
}

pub trait VfsPolicy: Send + Sync + 'static {
    fn check_path_op(
        &self,
        _ctx: &VfsContext,
        _op: VfsOp,
        _target: &VfsInodeId,
        _meta: Option<&VfsMetadata>,
    ) -> VfsResult<()> {
        Ok(())
    }

    fn check_open(
        &self,
        _ctx: &VfsContext,
        _target: &VfsInodeId,
        _opts: &OpenOptions,
        _meta: Option<&VfsMetadata>,
    ) -> VfsResult<()> {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct AllowAllPolicy;

impl VfsPolicy for AllowAllPolicy {}
