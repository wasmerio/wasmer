use crate::policy::VfsPolicy;
use crate::{VfsDirHandle, VfsGid, VfsHandleId, VfsUid};
use smallvec::SmallVec;
use std::sync::Arc;
use vfs_ratelimit::RateLimiter;

#[derive(Clone, Debug)]
pub struct VfsConfig {
    pub max_symlinks: u16,
    pub max_path_len: usize,
    pub max_name_len: usize,
}

impl Default for VfsConfig {
    fn default() -> Self {
        Self {
            max_symlinks: 40,
            max_path_len: 4096,
            max_name_len: 255,
        }
    }
}

#[derive(Clone, Debug)]
pub struct VfsCred {
    pub uid: VfsUid,
    pub gid: VfsGid,
    pub groups: SmallVec<[VfsGid; 8]>,
    pub umask: u32,
}

impl VfsCred {
    pub fn root() -> Self {
        Self {
            uid: 0,
            gid: 0,
            groups: SmallVec::new(),
            umask: 0,
        }
    }
}

/// Per-call context.
#[derive(Clone)]
pub struct VfsContext {
    pub cred: VfsCred,
    pub cwd: VfsDirHandle,
    pub config: Arc<VfsConfig>,
    pub policy: Arc<dyn VfsPolicy>,
    pub rate_limiter: Option<Arc<dyn RateLimiter>>,
}

impl VfsContext {
    pub fn new(
        cred: VfsCred,
        cwd: VfsDirHandle,
        config: Arc<VfsConfig>,
        policy: Arc<dyn VfsPolicy>,
    ) -> Self {
        Self {
            cred,
            cwd,
            config,
            policy,
            rate_limiter: None,
        }
    }

    pub fn cwd_handle_id(&self) -> VfsHandleId {
        self.cwd.id()
    }
}
