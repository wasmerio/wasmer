//! Filesystem instances, mounts and the mount tree (Linux's `fs/namespace.c` model).

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use std::sync::Weak;

use crate::{
    dentry::Dentry,
    driver::Filesystem,
    inode::Inode,
    types::{Device, FsInfo},
};

/// One filesystem instance, however many times it is mounted.
pub struct Superblock {
    pub id: u64,
    /// `st_dev` of its objects unless a driver reports one per object.
    pub dev: Device,
    pub fs: Arc<dyn Filesystem>,
    pub info: FsInfo,
    /// Serializes renames across directories, so the "not into its own subdirectory" check
    /// stays true until the driver has moved the entry (`s_vfs_rename_mutex`).
    pub(crate) rename_lock: futures_util::lock::Mutex<()>,
    /// The inode cache: every inode something holds, by number.
    pub(crate) inodes: Mutex<HashMap<u64, Weak<Inode>>>,
    /// The `st_dev` given to each device the driver reports per object (`Attr::dev`).
    pub(crate) sub_devs: Mutex<HashMap<Device, Device>>,
}

/// Per-mount flags (`MNT_*`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MountFlags {
    pub read_only: bool,
    pub nosuid: bool,
    pub nodev: bool,
    pub noexec: bool,
    pub noatime: bool,
    pub nosymfollow: bool,
}

/// A filesystem (or a subtree of one, for a bind mount) attached somewhere.
pub struct Mount {
    pub id: u64,
    /// Where the mount's tree starts: the filesystem root, or any directory for a bind mount.
    pub root: Arc<Dentry>,
    pub flags: MountFlags,
    /// The mount and dentry this one is attached on; `None` for the namespace root.
    pub(crate) parent: Mutex<Option<Path>>,
}

impl Mount {
    pub fn sb(&self) -> &Arc<Superblock> {
        self.root.sb()
    }

    /// `__mnt_is_readonly`: the mount or the filesystem is read-only.
    pub fn is_read_only(&self) -> bool {
        self.flags.read_only || self.sb().info.read_only
    }

    pub fn parent(&self) -> Option<Path> {
        self.parent.lock().unwrap().clone()
    }
}

/// A location in the tree: a mount and a dentry inside it.
#[derive(Clone)]
pub struct Path {
    pub mnt: Arc<Mount>,
    pub dentry: Arc<Dentry>,
}

impl PartialEq for Path {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.mnt, &other.mnt) && Arc::ptr_eq(&self.dentry, &other.dentry)
    }
}

impl Eq for Path {}

impl std::fmt::Debug for Path {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Path")
            .field("mnt", &self.mnt.id)
            .field("name", &String::from_utf8_lossy(&self.dentry.name()))
            .finish()
    }
}

impl Path {
    pub fn is_mount_root(&self) -> bool {
        Arc::ptr_eq(&self.dentry, &self.mnt.root)
    }
}

type MountKey = (u64, usize);

fn key(path: &Path) -> MountKey {
    (path.mnt.id, Arc::as_ptr(&path.dentry) as usize)
}

/// A mount tree (a mount namespace).
pub struct Namespace {
    root: Arc<Mount>,
    /// Mounts by the place they are attached; the last one of each list is on top.
    children: Mutex<HashMap<MountKey, Vec<Arc<Mount>>>>,
}

impl Namespace {
    pub(crate) fn new(root: Arc<Mount>) -> Self {
        Self {
            root,
            children: Mutex::default(),
        }
    }

    /// The root of the tree.
    pub fn root(&self) -> Path {
        Path {
            mnt: self.root.clone(),
            dentry: self.root.root.clone(),
        }
    }

    /// Attaches `mount` on `at`, over anything already mounted there.
    pub(crate) fn attach(&self, mount: Arc<Mount>, at: &Path) {
        *mount.parent.lock().unwrap() = Some(at.clone());
        at.dentry.add_mount(1);
        let mut children = self.children.lock().unwrap();
        children.entry(key(at)).or_default().push(mount);
    }

    /// Detaches the topmost mount on `at` (`umount`).
    pub(crate) fn detach(&self, at: &Path) -> Option<Arc<Mount>> {
        let mut children = self.children.lock().unwrap();
        let stack = children.get_mut(&key(at))?;
        let mount = stack.pop()?;
        if stack.is_empty() {
            children.remove(&key(at));
        }
        at.dentry.add_mount(-1);
        Some(mount)
    }

    /// The mount on top of `path`, if any (`lookup_mnt`).
    pub(crate) fn mounted_on(&self, path: &Path) -> Option<Arc<Mount>> {
        if !path.dentry.is_mountpoint() {
            return None;
        }
        let children = self.children.lock().unwrap();
        children.get(&key(path))?.last().cloned()
    }

    /// Follows mounts down from `path` to the root of the topmost one (`traverse_mounts`).
    /// Returns whether a mount was crossed.
    pub(crate) fn follow_mounts(&self, path: &mut Path) -> bool {
        let mut crossed = false;
        while let Some(mount) = self.mounted_on(path) {
            path.dentry = mount.root.clone();
            path.mnt = mount;
            crossed = true;
        }
        crossed
    }

    /// Every mount in the tree, parents before children.
    pub fn mounts(&self) -> Vec<Arc<Mount>> {
        let children = self.children.lock().unwrap();
        let mut all = vec![self.root.clone()];
        let mut i = 0;
        while i < all.len() {
            let parent_id = all[i].id;
            let mut found: Vec<_> = children
                .iter()
                .filter(|((id, _), _)| *id == parent_id)
                .flat_map(|(_, stack)| stack.iter().cloned())
                .collect();
            found.sort_by_key(|mount| mount.id);
            all.extend(found);
            i += 1;
        }
        all
    }
}
