//! Policy hooks (allow/deny, confinement, etc.).

use crate::VfsResult;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VfsOperation {
    PathResolve,
    Open,
    Stat,
    ReadDir,
    Read,
    Write,
    Create,
    Unlink,
    Rmdir,
    Mkdir,
    Rename,
    Link,
    Symlink,
    Readlink,
    SetMetadata,
}

pub trait VfsPolicy: Send + Sync + 'static {
    fn check(&self, _op: VfsOperation) -> VfsResult<()> {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct AllowAllPolicy;

impl VfsPolicy for AllowAllPolicy {}
