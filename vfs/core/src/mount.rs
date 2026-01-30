//! Mount table and mount semantics.

use crate::inode::make_vfs_inode;
use crate::provider::MountFlags;
use crate::{BackendInodeId, Fs, MountId, VfsError, VfsErrorKind, VfsInodeId, VfsResult};
use smallvec::SmallVec;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MountState {
    Active = 1,
    Detached = 2,
}

impl MountState {
    fn from_u8(value: u8) -> Self {
        match value {
            2 => MountState::Detached,
            _ => MountState::Active,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnmountFlags {
    None,
    Detach,
}

pub struct MountEntry {
    pub id: MountId,
    pub parent: Option<MountId>,
    pub mountpoint: Option<VfsInodeId>,
    pub root_inode: VfsInodeId,
    pub fs: Arc<dyn Fs>,
    pub flags: MountFlags,
    state: AtomicU8,
    open_count: AtomicU64,
}

impl MountEntry {
    fn new(
        id: MountId,
        parent: Option<MountId>,
        mountpoint: Option<VfsInodeId>,
        root_inode: VfsInodeId,
        fs: Arc<dyn Fs>,
        flags: MountFlags,
    ) -> Self {
        Self {
            id,
            parent,
            mountpoint,
            root_inode,
            fs,
            flags,
            state: AtomicU8::new(MountState::Active as u8),
            open_count: AtomicU64::new(0),
        }
    }

    pub fn state(&self) -> MountState {
        MountState::from_u8(self.state.load(Ordering::Acquire))
    }
}

#[derive(Clone)]
pub struct MountTable {
    inner: Arc<RwLock<Arc<MountTableInner>>>,
}

pub struct MountTableInner {
    pub root: MountId,
    pub mounts: Vec<Option<Arc<MountEntry>>>,
    pub mount_by_mountpoint: HashMap<VfsInodeId, MountId>,
    pub children_by_parent: HashMap<MountId, SmallVec<[MountId; 4]>>,
}

impl Clone for MountTableInner {
    fn clone(&self) -> Self {
        Self {
            root: self.root,
            mounts: self.mounts.clone(),
            mount_by_mountpoint: self.mount_by_mountpoint.clone(),
            children_by_parent: self.children_by_parent.clone(),
        }
    }
}

impl MountTable {
    pub fn new(root_fs: Arc<dyn Fs>) -> VfsResult<Self> {
        let root_id = MountId::from_index(0);
        let root_node = root_fs.root();
        let root_inode = make_vfs_inode(root_id, root_node.inode());

        let mut mounts = Vec::with_capacity(4);
        mounts.push(Some(Arc::new(MountEntry::new(
            root_id,
            None,
            None,
            root_inode,
            root_fs,
            MountFlags::empty(),
        ))));

        let inner = MountTableInner {
            root: root_id,
            mounts,
            mount_by_mountpoint: HashMap::new(),
            children_by_parent: HashMap::new(),
        };

        Ok(Self {
            inner: Arc::new(RwLock::new(Arc::new(inner))),
        })
    }

    pub fn snapshot(&self) -> Arc<MountTableInner> {
        self.inner
            .read()
            .expect("mount table lock poisoned")
            .clone()
    }

    pub fn enter_if_mountpoint(
        inner: &MountTableInner,
        _current_mount: MountId,
        child_inode: VfsInodeId,
    ) -> Option<MountId> {
        let mount = inner.mount_by_mountpoint.get(&child_inode).copied()?;
        let entry = inner.mounts.get(mount.index())?.as_ref()?;
        if entry.state() == MountState::Active {
            Some(mount)
        } else {
            None
        }
    }

    pub fn mount_root(
        inner: &MountTableInner,
        mount: MountId,
    ) -> Option<(VfsInodeId, Arc<dyn Fs>)> {
        let entry = inner.mounts.get(mount.index())?.as_ref()?;
        if entry.state() == MountState::Active {
            Some((entry.root_inode, entry.fs.clone()))
        } else {
            None
        }
    }

    pub fn mount_root_any(
        inner: &MountTableInner,
        mount: MountId,
    ) -> Option<(VfsInodeId, Arc<dyn Fs>)> {
        let entry = inner.mounts.get(mount.index())?.as_ref()?;
        Some((entry.root_inode, entry.fs.clone()))
    }

    pub fn parent_of_mount_root(
        inner: &MountTableInner,
        mount: MountId,
    ) -> Option<(MountId, VfsInodeId)> {
        let entry = inner.mounts.get(mount.index())?.as_ref()?;
        let parent = entry.parent?;
        let mountpoint = entry.mountpoint?;
        Some((parent, mountpoint))
    }

    pub fn mount(
        &self,
        parent_mount: MountId,
        mountpoint_inode: VfsInodeId,
        fs: Arc<dyn Fs>,
        root_inode: BackendInodeId,
        flags: MountFlags,
    ) -> VfsResult<MountId> {
        if mountpoint_inode.mount != parent_mount {
            return Err(VfsError::new(
                VfsErrorKind::InvalidInput,
                "mount.parent_mismatch",
            ));
        }

        let mut guard = self
            .inner
            .write()
            .map_err(|_| VfsError::new(VfsErrorKind::Internal, "mount.lock"))?;
        let mut inner = (**guard).clone();

        if inner.mount_by_mountpoint.contains_key(&mountpoint_inode) {
            return Err(VfsError::new(VfsErrorKind::AlreadyExists, "mount.exists"));
        }

        if inner
            .mounts
            .get(parent_mount.index())
            .and_then(|slot| slot.as_ref())
            .filter(|entry| entry.state() == MountState::Active)
            .is_none()
        {
            return Err(VfsError::new(
                VfsErrorKind::NotFound,
                "mount.parent_not_found",
            ));
        }

        let (entry_id, entry) = {
            let mut slot_index = None;
            for (idx, slot) in inner.mounts.iter().enumerate() {
                if slot.is_none() {
                    slot_index = Some(idx);
                    break;
                }
            }
            let idx = slot_index.unwrap_or_else(|| inner.mounts.len());
            let id = MountId::from_index(idx);
            let root_inode = make_vfs_inode(id, root_inode);
            let entry = Arc::new(MountEntry::new(
                id,
                Some(parent_mount),
                Some(mountpoint_inode),
                root_inode,
                fs,
                flags,
            ));
            (id, entry)
        };

        if entry_id.index() == inner.mounts.len() {
            inner.mounts.push(Some(entry.clone()));
        } else {
            inner.mounts[entry_id.index()] = Some(entry.clone());
        }

        inner.mount_by_mountpoint.insert(mountpoint_inode, entry_id);
        inner
            .children_by_parent
            .entry(parent_mount)
            .or_default()
            .push(entry_id);

        *guard = Arc::new(inner);
        Ok(entry_id)
    }

    pub fn unmount(&self, target_mount: MountId, flags: UnmountFlags) -> VfsResult<()> {
        let mut guard = self
            .inner
            .write()
            .map_err(|_| VfsError::new(VfsErrorKind::Internal, "unmount.lock"))?;
        let mut inner = (**guard).clone();

        if target_mount == inner.root {
            return Err(VfsError::new(
                VfsErrorKind::InvalidInput,
                "unmount.root_forbidden",
            ));
        }

        let entry = inner
            .mounts
            .get(target_mount.index())
            .and_then(|slot| slot.as_ref())
            .ok_or(VfsError::new(VfsErrorKind::NotFound, "unmount.not_found"))?;

        if inner
            .children_by_parent
            .get(&target_mount)
            .map(|children| !children.is_empty())
            .unwrap_or(false)
        {
            return Err(VfsError::new(VfsErrorKind::Busy, "unmount.has_children"));
        }

        if entry.open_count.load(Ordering::Acquire) > 0 {
            if matches!(flags, UnmountFlags::Detach) {
                entry
                    .state
                    .store(MountState::Detached as u8, Ordering::Release);
            } else {
                return Err(VfsError::new(VfsErrorKind::Busy, "unmount.busy"));
            }
        }

        if let Some(mountpoint) = entry.mountpoint {
            inner.mount_by_mountpoint.remove(&mountpoint);
        }

        if let Some(parent) = entry.parent {
            if let Some(children) = inner.children_by_parent.get_mut(&parent) {
                children.retain(|child| *child != target_mount);
            }
        }

        if entry.open_count.load(Ordering::Acquire) == 0 {
            inner.mounts[target_mount.index()] = None;
            inner.children_by_parent.remove(&target_mount);
        }

        *guard = Arc::new(inner);
        Ok(())
    }

    pub fn guard(&self, mount: MountId) -> VfsResult<MountGuard> {
        let inner = self.snapshot();
        let entry = inner
            .mounts
            .get(mount.index())
            .and_then(|slot| slot.as_ref())
            .ok_or(VfsError::new(VfsErrorKind::NotFound, "mount.guard"))?;
        if entry.state() != MountState::Active {
            return Err(VfsError::new(VfsErrorKind::NotFound, "mount.detached"));
        }
        entry.open_count.fetch_add(1, Ordering::AcqRel);
        Ok(MountGuard {
            entry: entry.clone(),
            mount_table: self.clone(),
        })
    }

    pub fn reclaim_detached(&self, mount: MountId) {
        let mut guard = match self.inner.write() {
            Ok(guard) => guard,
            Err(_) => return,
        };
        let mut inner = (**guard).clone();
        let entry = match inner
            .mounts
            .get(mount.index())
            .and_then(|slot| slot.as_ref())
        {
            Some(entry) => entry,
            None => return,
        };

        if entry.state() != MountState::Detached || entry.open_count.load(Ordering::Acquire) != 0 {
            return;
        }

        if let Some(mountpoint) = entry.mountpoint {
            inner.mount_by_mountpoint.remove(&mountpoint);
        }

        inner.mounts[mount.index()] = None;
        inner.children_by_parent.remove(&mount);
        *guard = Arc::new(inner);
    }
}

pub struct MountGuard {
    entry: Arc<MountEntry>,
    mount_table: MountTable,
}

impl std::fmt::Debug for MountGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MountGuard")
            .field("mount", &self.entry.id)
            .finish()
    }
}

impl Clone for MountGuard {
    fn clone(&self) -> Self {
        self.entry.open_count.fetch_add(1, Ordering::AcqRel);
        Self {
            entry: self.entry.clone(),
            mount_table: self.mount_table.clone(),
        }
    }
}

impl Drop for MountGuard {
    fn drop(&mut self) {
        let previous = self.entry.open_count.fetch_sub(1, Ordering::AcqRel);
        if previous == 1 && self.entry.state() == MountState::Detached {
            self.mount_table.reclaim_detached(self.entry.id);
        }
    }
}
