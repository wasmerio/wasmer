//! Filesystem quota operations (`struct quotactl_ops`, fs/quota/quota.c).

use wasixv2_abi::{IfDqblk, IfDqinfo};

use crate::FsResult;

/// Quota accounting belongs to the filesystem, as does enforcement at allocation time.
/// The kernel checks credentials and copies the syscall's type-dependent user argument.
pub trait QuotaOps: Send + Sync {
    /// Whether this quota type has accounting enabled (`sb_has_quota_active`).
    fn active(&self, kind: usize) -> bool;
    /// Changes enforcement, retaining accounting (`dquot_quota_enable/disable`).
    fn enforce(&self, kind: usize, enabled: bool) -> FsResult<()>;
    fn get(&self, kind: usize, id: u32) -> FsResult<IfDqblk>;
    fn next(&self, kind: usize, id: u32) -> FsResult<(u32, IfDqblk)>;
    fn set(&self, kind: usize, id: u32, limits: &IfDqblk) -> FsResult<()>;
    fn info(&self, kind: usize) -> FsResult<IfDqinfo>;
    fn set_info(&self, kind: usize, info: &IfDqinfo) -> FsResult<()>;
}
