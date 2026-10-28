//! Filesystem instances, mounts, mount namespaces and the mount table (Linux's `fs/mount.h`
//! and the data side of `fs/namespace.c`).
//!
//! Every mount of a VFS lives in one table ([`Tree`], Linux's `mount_hashtable` with
//! `namespace_sem`), whatever namespace it belongs to: mounts are never shared between
//! namespaces (`copy_mnt_ns` clones them), so a walk finds what is mounted on a place by the
//! mount and dentry alone and needs no namespace.

use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, MutexGuard, RwLock, Weak,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use crate::{
    dentry::Dentry,
    driver::{Filesystem, Node},
    inode::Inode,
    pnode::Link,
    sync::{MutexExt, RwLockExt},
    types::{Device, FsInfo},
};

/// One filesystem instance, however many times it is mounted.
pub struct Superblock {
    pub id: u64,
    /// `st_dev` of its objects unless a driver reports one per object.
    pub dev: Device,
    pub fs: Arc<dyn Filesystem>,
    pub info: FsInfo,
    /// `SB_RDONLY`: starts as the driver says, and a remount changes it, unless the driver
    /// cannot write at all.
    pub(crate) read_only: AtomicBool,
    /// Serializes renames across directories, so the "not into its own subdirectory" check
    /// stays true until the driver has moved the entry (`s_vfs_rename_mutex`).
    pub(crate) rename_lock: futures_util::lock::Mutex<()>,
    /// The VFS's count of changes to the shape of its tree.
    pub(crate) tree_seq: Arc<TreeSeq>,
    /// The inode cache: every inode something holds, by number.
    pub(crate) inodes: Mutex<HashMap<u64, Weak<Inode>>>,
    /// The `st_dev` given to each device the driver reports per object (`Attr::dev`).
    pub(crate) sub_devs: Mutex<HashMap<Device, Device>>,
    /// The dentry of the filesystem's root while something holds it (`s_root`), so every
    /// mount of the filesystem starts at the same one.
    pub(crate) root: Mutex<Weak<Dentry>>,
}

impl Superblock {
    /// `sb_rdonly`.
    pub fn is_read_only(&self) -> bool {
        self.read_only.load(Ordering::Relaxed)
    }

    /// Makes the filesystem read-only or writable (`reconfigure_super` with `SB_RDONLY`). A
    /// driver that cannot write stays read-only, as filesystems that force `SB_RDONLY` do.
    pub(crate) fn set_read_only(&self, read_only: bool) {
        self.read_only
            .store(read_only || self.info.read_only, Ordering::Relaxed);
    }
}

/// Counts the changes that move a name to another place in a VFS's tree, a dentry move or a
/// mount or unmount (Linux's `rename_lock` and `mount_lock` sequence counts), so a walk scoped
/// to a directory can tell that `..` might have left it through a concurrent change.
#[derive(Default)]
pub(crate) struct TreeSeq(AtomicU64);

impl TreeSeq {
    /// The current count.
    pub(crate) fn read(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }

    /// Counts a change. Called before the change, so a walk that sees the change sees the
    /// count go up too.
    pub(crate) fn bump(&self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

/// Per-mount flags (`MNT_*`). Access times are updated as `relatime` says unless `noatime` or
/// `strictatime` is set.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MountFlags {
    pub read_only: bool,
    pub nosuid: bool,
    pub nodev: bool,
    pub noexec: bool,
    pub noatime: bool,
    /// `MNT_NODIRATIME`.
    pub nodiratime: bool,
    /// Neither `MNT_RELATIME` nor `MNT_NOATIME`.
    pub strictatime: bool,
    pub nosymfollow: bool,
}

/// A filesystem (or a subtree of one, for a bind mount) attached somewhere.
pub struct Mount {
    pub id: u64,
    /// Where the mount's tree starts: the filesystem root, or any directory for a bind mount.
    pub root: Arc<Dentry>,
    flags: RwLock<MountFlags>,
    /// What the mount was made from (`mnt_devname`), as `/proc/<pid>/mounts` shows it.
    devname: Vec<u8>,
    /// The mount and dentry this one is attached on; `None` for a namespace root and for a
    /// mount no longer in any tree.
    pub(crate) parent: Mutex<Option<Path>>,
    /// `MNT_EXPIRE` marked it (`mnt_expiry_mark`).
    pub(crate) expiry_mark: AtomicBool,
}

impl Mount {
    pub(crate) fn new(id: u64, root: Arc<Dentry>, flags: MountFlags, devname: Vec<u8>) -> Self {
        Self {
            id,
            root,
            flags: RwLock::new(flags),
            devname,
            parent: Mutex::new(None),
            expiry_mark: AtomicBool::new(false),
        }
    }

    pub fn sb(&self) -> &Arc<Superblock> {
        self.root.sb()
    }

    /// The per-mount flags as they are now.
    pub fn flags(&self) -> MountFlags {
        *self.flags.read_or_recover()
    }

    pub(crate) fn set_flags(&self, flags: MountFlags) {
        *self.flags.write_or_recover() = flags;
    }

    /// The source the mount was made from.
    pub fn devname(&self) -> &[u8] {
        &self.devname
    }

    /// `__mnt_is_readonly`: the mount or the filesystem is read-only.
    pub fn is_read_only(&self) -> bool {
        self.flags().read_only || self.sb().is_read_only()
    }

    pub fn parent(&self) -> Option<Path> {
        self.parent.lock_or_recover().clone()
    }

    /// A path for `node`, an object of this mount's filesystem that no directory holds, under
    /// `name` (`d_alloc_pseudo`). `d_path` shows it as the filesystem's `dname` says, or as
    /// `/<name> (deleted)` (`simple_dname`).
    pub fn pseudo_path(self: &Arc<Self>, node: Arc<dyn Node>, name: &[u8]) -> Path {
        Path {
            mnt: self.clone(),
            dentry: Dentry::pseudo(self.sb(), node, name),
        }
    }

    /// The root of the mount as a path.
    pub fn root_path(self: &Arc<Self>) -> Path {
        Path {
            mnt: self.clone(),
            dentry: self.root.clone(),
        }
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
    /// `path_mounted`: the path is the root of its mount.
    pub fn is_mount_root(&self) -> bool {
        Arc::ptr_eq(&self.dentry, &self.mnt.root)
    }
}

pub(crate) type MountKey = (u64, usize);

pub(crate) fn key(path: &Path) -> MountKey {
    (path.mnt.id, Arc::as_ptr(&path.dentry) as usize)
}

/// A mount namespace (`struct mnt_namespace`): the tree of mounts below its root. Dropping the
/// last reference unmounts all of it (`put_mnt_ns`), though mounts something still holds stay
/// usable, detached, as after a lazy unmount.
pub struct MntNs {
    id: u64,
    root: Arc<Mount>,
    pub(crate) tree: Arc<Tree>,
}

impl MntNs {
    pub(crate) fn new(tree: &Arc<Tree>, root: Arc<Mount>) -> Arc<Self> {
        let id = tree.next_id.fetch_add(1, Ordering::Relaxed);
        Arc::new(Self {
            id,
            root,
            tree: tree.clone(),
        })
    }

    /// The namespace's identifier, unique in its VFS (`mnt_namespace.ns.ns_id`).
    pub fn id(&self) -> u64 {
        self.id
    }

    /// The root of the tree (`ns->root`), which nothing can unmount or move.
    pub fn root(&self) -> Path {
        self.root.root_path()
    }

    /// The namespace's mounts, oldest first.
    pub fn mounts(&self) -> Vec<Arc<Mount>> {
        self.tree.lock().mounts_in(self.id)
    }

    /// Whether `mount` is attached in this namespace (`check_mnt`).
    pub fn contains(&self, mount: &Mount) -> bool {
        self.tree.lock().ns_of(mount.id) == Some(self.id)
    }
}

impl Drop for MntNs {
    fn drop(&mut self) {
        // The mounts go after the lock: dropping one may drop paths that hold others.
        let _gone = {
            let mut state = self.tree.lock();
            state.umount_tree(&self.root, false)
        };
        self.tree.seq.bump();
    }
}

/// The mount table of one VFS.
pub(crate) struct Tree {
    state: Mutex<TreeState>,
    pub(crate) seq: Arc<TreeSeq>,
    /// Identifiers of mounts, superblocks, namespaces and open files.
    pub(crate) next_id: AtomicU64,
}

impl Tree {
    pub(crate) fn new(seq: Arc<TreeSeq>) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::default(),
            seq,
            next_id: AtomicU64::new(1),
        })
    }

    pub(crate) fn next_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    /// Takes the table (`namespace_sem` and `mount_lock` at once).
    pub(crate) fn lock(&self) -> MutexGuard<'_, TreeState> {
        self.state.lock_or_recover()
    }

    /// The mount on top of `path`, if any (`lookup_mnt`).
    pub(crate) fn mounted_on(&self, path: &Path) -> Option<Arc<Mount>> {
        if !path.dentry.is_mountpoint() {
            return None;
        }
        self.lock().children.get(&key(path)).cloned()
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
}

/// The mount table's contents.
#[derive(Default)]
pub(crate) struct TreeState {
    /// The mount on each mountpoint, by the mount and dentry it is attached on.
    pub(crate) children: HashMap<MountKey, Arc<Mount>>,
    /// The namespace and propagation of every mount in a tree, attached or being built, by id.
    pub(crate) links: HashMap<u64, Link>,
}

impl TreeState {
    /// The namespace `id` is attached in.
    pub(crate) fn ns_of(&self, id: u64) -> Option<u64> {
        self.links
            .get(&id)
            .map(|link| link.ns)
            .filter(|&ns| ns != 0)
    }

    /// Attaches `mount` on `at` (`mnt_set_mountpoint` and `commit_tree`). Nothing is on `at`.
    pub(crate) fn attach(&mut self, mount: &Arc<Mount>, at: &Path) {
        *mount.parent.lock_or_recover() = Some(at.clone());
        at.dentry.add_mount(1);
        let previous = self.children.insert(key(at), mount.clone());
        debug_assert!(previous.is_none(), "one mount per mountpoint");
    }

    /// Takes `mount` off its mountpoint (`umount_mnt`), returning where it was.
    pub(crate) fn detach(&mut self, mount: &Arc<Mount>) -> Option<Path> {
        let at = mount.parent.lock_or_recover().take()?;
        if self
            .children
            .get(&key(&at))
            .is_some_and(|on| Arc::ptr_eq(on, mount))
        {
            self.children.remove(&key(&at));
        }
        at.dentry.add_mount(-1);
        Some(at)
    }

    /// The mount on `path`, if any.
    pub(crate) fn mounted_on(&self, path: &Path) -> Option<Arc<Mount>> {
        if !path.dentry.is_mountpoint() {
            return None;
        }
        self.children.get(&key(path)).cloned()
    }

    /// The topmost of the mounts stacked on `mount`'s root (`topmost_overmount`).
    pub(crate) fn topmost(&self, mount: &Arc<Mount>) -> Arc<Mount> {
        let mut top = mount.clone();
        while let Some(over) = self.mounted_on(&top.root_path()) {
            top = over;
        }
        top
    }

    /// Where a mount on `path` goes (`where_to_mount`): on top of whatever covers it.
    pub(crate) fn where_to_mount(&self, path: &Path) -> Path {
        match self.mounted_on(path) {
            Some(over) => self.topmost(&over).root_path(),
            None => path.clone(),
        }
    }

    /// The mounts attached on `mount`, oldest first (`mnt_mounts`).
    pub(crate) fn children_of(&self, mount: &Mount) -> Vec<Arc<Mount>> {
        let mut found: Vec<_> = self
            .children
            .iter()
            .filter(|((parent, _), _)| *parent == mount.id)
            .map(|(_, child)| child.clone())
            .collect();
        found.sort_by_key(|child| child.id);
        found
    }

    pub(crate) fn has_children(&self, mount: &Mount) -> bool {
        self.children.keys().any(|(parent, _)| *parent == mount.id)
    }

    /// `mount` and every mount below it, parents before children (`next_mnt` order).
    pub(crate) fn subtree(&self, mount: &Arc<Mount>) -> Vec<Arc<Mount>> {
        let mut out = Vec::new();
        self.preorder(mount, &mut out);
        out
    }

    fn preorder(&self, mount: &Arc<Mount>, out: &mut Vec<Arc<Mount>>) {
        out.push(mount.clone());
        for child in self.children_of(mount) {
            self.preorder(&child, out);
        }
    }

    /// Every mount attached in namespace `ns`, oldest first.
    pub(crate) fn mounts_in(&self, ns: u64) -> Vec<Arc<Mount>> {
        let mut found: Vec<_> = self
            .links
            .values()
            .filter(|link| link.ns == ns)
            .filter_map(|link| link.mount.upgrade())
            .collect();
        found.sort_by_key(|mount| mount.id);
        found
    }

    /// Whether some mount of namespace `ns` is attached on `dentry` (`is_local_mountpoint`).
    pub(crate) fn is_local_mountpoint(&self, ns: u64, dentry: &Arc<Dentry>) -> bool {
        if !dentry.is_mountpoint() {
            return false;
        }
        let ptr = Arc::as_ptr(dentry) as usize;
        self.children
            .keys()
            .any(|&(parent, at)| at == ptr && self.ns_of(parent) == Some(ns))
    }

    /// The mounts attached on `dentry` in any mount, any namespace.
    pub(crate) fn mounted_on_dentry(&self, dentry: &Arc<Dentry>) -> Vec<Arc<Mount>> {
        if !dentry.is_mountpoint() {
            return Vec::new();
        }
        let ptr = Arc::as_ptr(dentry) as usize;
        self.children
            .iter()
            .filter(|((_, at), _)| *at == ptr)
            .map(|(_, child)| child.clone())
            .collect()
    }
}

/// `is_path_reachable`: whether `dentry` in `mount` is at or below `root`, going up through
/// the mountpoints.
pub(crate) fn is_path_reachable(mount: &Arc<Mount>, dentry: &Arc<Dentry>, root: &Path) -> bool {
    let mut mnt = mount.clone();
    let mut dentry = dentry.clone();
    while !Arc::ptr_eq(&mnt, &root.mnt) {
        let Some(at) = mnt.parent() else {
            return false;
        };
        mnt = at.mnt;
        dentry = at.dentry;
    }
    root.dentry.is_ancestor_of(&dentry)
}
