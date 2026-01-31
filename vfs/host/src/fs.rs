use std::sync::Arc;

use vfs_core::provider::MountFlags;
use vfs_core::{BackendInodeId, VfsCapabilities, VfsResult};

use crate::config::HostFsConfig;
use crate::node::{HostDir, HostNode, HostNodeKind, Locator};
use crate::platform;

#[derive(Debug)]
pub struct HostFs {
    inner: Arc<HostFsInner>,
}

#[derive(Debug)]
pub(crate) struct HostFsInner {
    pub(crate) root: platform::DirHandle,
    pub(crate) root_inode: BackendInodeId,
    #[allow(dead_code)]
    pub(crate) config: HostFsConfig,
    pub(crate) mount_flags: MountFlags,
    pub(crate) caps: VfsCapabilities,
}

impl HostFs {
    pub fn new(config: HostFsConfig, mount_flags: MountFlags) -> VfsResult<Self> {
        let root = crate::io_result("host.open_root", platform::open_root_dir(&config.root))?;
        let root_stat = crate::io_result("host.stat_root", platform::stat_root(&root))?;
        let caps = Self::caps_for_mount(mount_flags);
        let inner = Arc::new(HostFsInner {
            root,
            root_inode: root_stat.inode,
            config,
            mount_flags,
            caps,
        });
        Ok(Self { inner })
    }

    fn caps_for_mount(mount_flags: MountFlags) -> VfsCapabilities {
        let mut caps = VfsCapabilities::NONE;
        let provider_caps = platform::provider_capabilities();
        if provider_caps.contains(vfs_core::provider::FsProviderCapabilities::SYMLINK) {
            caps = caps.union(VfsCapabilities::SYMLINKS);
        }
        if provider_caps.contains(vfs_core::provider::FsProviderCapabilities::HARDLINK) {
            caps = caps.union(VfsCapabilities::HARDLINKS);
        }
        if provider_caps.contains(vfs_core::provider::FsProviderCapabilities::UNIX_PERMISSIONS) {
            caps = caps
                .union(VfsCapabilities::CHMOD)
                .union(VfsCapabilities::CHOWN);
        }
        if provider_caps.contains(vfs_core::provider::FsProviderCapabilities::UTIMENS) {
            caps = caps.union(VfsCapabilities::UTIMENS);
        }
        if provider_caps.contains(vfs_core::provider::FsProviderCapabilities::RENAME_ATOMIC) {
            caps = caps.union(VfsCapabilities::RENAME_EXCHANGE);
        }
        let _ = mount_flags;
        caps
    }

    pub(crate) fn root_node(&self) -> Arc<HostNode> {
        let locator = Locator {
            parent: None,
            name: None,
        };
        let dir = HostDir {
            dir: self.inner.root.clone(),
            inode: self.inner.root_inode,
            locator,
        };
        Arc::new(HostNode {
            fs: self.inner.clone(),
            kind: HostNodeKind::Dir(Arc::new(dir)),
        })
    }
}

impl vfs_core::traits_sync::FsSync for HostFs {
    fn provider_name(&self) -> &'static str {
        "host"
    }

    fn capabilities(&self) -> VfsCapabilities {
        self.inner.caps
    }

    fn root(&self) -> Arc<dyn vfs_core::traits_sync::FsNodeSync> {
        self.root_node()
    }
}
