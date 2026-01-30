//! Inode model and identifiers.
//!
//! # Stability contract
//!
//! Within a single mounted filesystem instance:
//! - the same filesystem object should map to the same [`BackendInodeId`],
//! - ids remain stable across lookup/reopen/readdir/rename operations,
//! - ids must never be reused while the mount is alive.
//!
//! Ids do not need to survive unmount/remount or process restarts.

use crate::node::FsNode;
use crate::{BackendInodeId, MountId, VfsInodeId};
use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
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

    pub(crate) fn downgrade(&self) -> Weak<NodeRefInner> {
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

/// Backend object identity for synthetic inode mapping.
pub type InodeKey = Box<[u8]>;

/// Synthetic inode mapping helper for non-POSIX backends.
pub struct SyntheticInodeMap {
    next: AtomicU64,
    by_key: RwLock<HashMap<InodeKey, BackendInodeId>>,
}

impl SyntheticInodeMap {
    pub fn new() -> Self {
        Self {
            next: AtomicU64::new(1),
            by_key: RwLock::new(HashMap::new()),
        }
    }

    /// Get an existing inode id for `key` or allocate a fresh one.
    pub fn get_or_alloc(&self, key: &InodeKey) -> BackendInodeId {
        if let Some(id) = self.by_key.read().get(key).copied() {
            return id;
        }
        let mut guard = self.by_key.write();
        if let Some(id) = guard.get(key).copied() {
            return id;
        }
        let raw = self.next.fetch_add(1, Ordering::Relaxed);
        let id = BackendInodeId::new(raw).expect("non-zero inode");
        guard.insert(key.clone(), id);
        id
    }

    /// Forget a single key mapping (ids are never reused).
    pub fn invalidate(&self, key: &InodeKey) {
        self.by_key.write().remove(key);
    }

    /// Forget mappings with a given prefix (ids are never reused).
    pub fn invalidate_prefix(&self, prefix: &[u8]) {
        let mut guard = self.by_key.write();
        guard.retain(|key, _| !key.starts_with(prefix));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn vfs_inode_id_is_copy_hash_and_mount_scoped() {
        let mount_a = MountId::from_index(0);
        let mount_b = MountId::from_index(1);
        let ino = BackendInodeId::new(1).expect("non-zero inode");
        let a = make_vfs_inode(mount_a, ino);
        let b = make_vfs_inode(mount_b, ino);

        let mut set = HashSet::new();
        set.insert(a);
        set.insert(b);

        let _copy = a;
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn synthetic_inode_map_allocates_and_is_stable() {
        let map = SyntheticInodeMap::new();
        let key_a: InodeKey = Box::from(&b"alpha"[..]);
        let key_b: InodeKey = Box::from(&b"bravo"[..]);

        let a1 = map.get_or_alloc(&key_a);
        let a2 = map.get_or_alloc(&key_a);
        let b1 = map.get_or_alloc(&key_b);

        assert_eq!(a1, a2);
        assert_ne!(a1, b1);
        assert_ne!(a1.get(), 0);
        assert_ne!(b1.get(), 0);
    }
}
