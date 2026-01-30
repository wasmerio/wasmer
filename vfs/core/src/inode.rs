//! Inode model and identifiers.

use crate::node::FsNode;
use crate::{BackendInodeId, MountId, VfsInodeId};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::{Arc, Weak};

#[inline]
pub fn make_vfs_inode(mount: MountId, backend: BackendInodeId) -> VfsInodeId {
    VfsInodeId { mount, backend }
}

#[inline]
pub fn split(vfs: VfsInodeId) -> (MountId, BackendInodeId) {
    (vfs.mount, vfs.backend)
}

#[derive(Clone)]
pub struct NodeRef {
    inner: Arc<NodeRefInner>,
}

struct NodeRefInner {
    mount: MountId,
    node: Arc<dyn FsNode>,
}

impl NodeRef {
    pub fn new(mount: MountId, node: Arc<dyn FsNode>) -> Self {
        Self {
            inner: Arc::new(NodeRefInner { mount, node }),
        }
    }

    pub fn mount(&self) -> MountId {
        self.inner.mount
    }

    pub fn node(&self) -> &Arc<dyn FsNode> {
        &self.inner.node
    }

    pub fn inode_id(&self) -> VfsInodeId {
        make_vfs_inode(self.inner.mount, self.inner.node.inode())
    }

    pub fn downgrade(&self) -> Weak<NodeRefInner> {
        Arc::downgrade(&self.inner)
    }
}

pub struct InodeCache {
    nodes: Mutex<HashMap<VfsInodeId, Weak<NodeRefInner>>>,
}

impl InodeCache {
    pub fn new() -> Self {
        Self {
            nodes: Mutex::new(HashMap::new()),
        }
    }

    pub fn get_or_insert<F>(&self, inode: VfsInodeId, factory: F) -> NodeRef
    where
        F: FnOnce() -> NodeRef,
    {
        let mut nodes = self.nodes.lock();
        if let Some(existing) = nodes.get(&inode).and_then(|weak| weak.upgrade()) {
            return NodeRef { inner: existing };
        }

        let created = factory();
        nodes.insert(inode, created.downgrade());
        created
    }
}
