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
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
};

use wasixv2_abi::Errno;

use crate::{
    cred::UserNs,
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
    /// The user namespace the filesystem belongs to (`s_user_ns`): its mounter's, or the
    /// initial one for what the embedder mounts. Only ids it maps can own what is made on it.
    pub owner: Arc<dyn UserNs>,
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
    /// Write accesses held on any mount of the filesystem (the sum of its mounts'
    /// `mnt_writers`), which a remount read-only waits for none of (`sb_prepare_remount_readonly`).
    pub(crate) writers: AtomicUsize,
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

/// What a mount copied into the mount namespace of a less privileged user namespace may not
/// change or uncover (`lock_mnt_tree`).
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct MountLock {
    /// `MNT_LOCKED`: it hides what is below it, so it stays where it is.
    pub(crate) covering: bool,
    /// The flags as they were locked (`MNT_LOCK_READONLY`, `MNT_LOCK_NODEV`, `MNT_LOCK_NOSUID`,
    /// `MNT_LOCK_NOEXEC`, `MNT_LOCK_ATIME`): those set stay set, and the atime mode stays.
    pub(crate) flags: Option<MountFlags>,
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
    /// Write accesses held through it (`mnt_writers`): files open for writing.
    pub(crate) writers: AtomicUsize,
    pub(crate) lock: RwLock<MountLock>,
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
            writers: AtomicUsize::new(0),
            lock: RwLock::default(),
        }
    }

    /// `MNT_LOCKED`: the mount was copied into a less privileged mount namespace over what it
    /// hides, so it cannot be unmounted or moved there.
    pub fn is_locked(&self) -> bool {
        self.lock.read_or_recover().covering
    }

    /// `can_change_locked_flags`: whether `new` keeps every flag the mount's lock holds.
    pub fn may_set_flags(&self, new: &MountFlags) -> bool {
        let Some(locked) = self.lock.read_or_recover().flags else {
            return true;
        };
        let atime = |f: &MountFlags| (f.noatime, f.nodiratime, f.strictatime);
        !(locked.read_only && !new.read_only
            || locked.nodev && !new.nodev
            || locked.nosuid && !new.nosuid
            || locked.noexec && !new.noexec
            || atime(&locked) != atime(new))
    }

    /// The id `statx(STATX_MNT_ID_UNIQUE)`, `statmount` and `listmount` use (`mnt_id_unique`):
    /// past the reused ids, from `MNT_UNIQUE_ID_OFFSET`. Mount ids are never reused here, so
    /// the unique id is the id moved past that offset.
    pub fn unique_id(&self) -> u64 {
        MNT_UNIQUE_ID_OFFSET + self.id
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

/// `MNT_UNIQUE_ID_OFFSET`: unique mount ids are above it, so they never look like the ids
/// `mountinfo` shows.
pub const MNT_UNIQUE_ID_OFFSET: u64 = 1 << 31;

/// Write access to a mount's filesystem, held by a file open for writing (`FMODE_WRITER`) from
/// `mnt_get_write_access` to `mnt_put_write_access`. While any is held, the mount cannot become
/// read-only, nor can its filesystem.
pub(crate) struct WriteAccess(Arc<Mount>);

impl Drop for WriteAccess {
    fn drop(&mut self) {
        self.0.writers.fetch_sub(1, Ordering::SeqCst);
        self.0.sb().writers.fetch_sub(1, Ordering::SeqCst);
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
    /// An anonymous namespace (`is_anon`), holding a detached tree that `open_tree` copied or
    /// `fsmount` made, with the namespace that tree came from (`seq_origin`; 0 for none).
    anon: Option<u64>,
    /// The user namespace that owns it (`mnt_ns->user_ns`).
    owner: Arc<dyn UserNs>,
}

impl MntNs {
    /// The user namespace that owns the namespace (`mnt_ns->user_ns`).
    pub fn owner(&self) -> &Arc<dyn UserNs> {
        &self.owner
    }

    /// A namespace whose tree starts at `root`, anonymous if `anon` names its origin, owned by
    /// `owner`.
    pub(crate) fn new(
        tree: &Arc<Tree>,
        state: &mut TreeState,
        root: Arc<Mount>,
        anon: Option<u64>,
        owner: Arc<dyn UserNs>,
    ) -> Arc<Self> {
        let id = tree.next_id.fetch_add(1, Ordering::Relaxed);
        let ns = Arc::new(Self {
            id,
            root,
            tree: tree.clone(),
            anon,
            owner,
        });
        match anon {
            Some(origin) => {
                state.anon_ns.insert(id, origin);
            }
            None => {
                state.namespaces.insert(id, Arc::downgrade(&ns));
            }
        }
        ns
    }

    /// Whether this is a detached tree's namespace (`is_anon_ns`).
    pub fn is_anon(&self) -> bool {
        self.anon.is_some()
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

    /// The mount of this namespace whose unique id is `id` (`lookup_mnt_in_ns`).
    pub fn mount_by_unique_id(&self, id: u64) -> Option<Arc<Mount>> {
        let mount = id.checked_sub(MNT_UNIQUE_ID_OFFSET)?;
        let state = self.tree.lock();
        state
            .links
            .get(&mount)
            .filter(|link| link.ns == self.id)
            .and_then(|link| link.mount.upgrade())
    }

    /// Whether `mount` is attached in this namespace (`check_mnt`).
    pub fn contains(&self, mount: &Mount) -> bool {
        self.tree.lock().ns_of(mount.id) == Some(self.id)
    }
}

impl Drop for MntNs {
    /// `put_mnt_ns`, and for an anonymous namespace `dissolve_on_fput`: what is still in the
    /// namespace goes. A detached tree that was attached somewhere has left it.
    fn drop(&mut self) {
        // The mounts go after the lock: dropping one may drop paths that hold others.
        let _gone = {
            let mut state = self.tree.lock();
            state.namespaces.remove(&self.id);
            state.anon_ns.remove(&self.id);
            match state.ns_of(self.root.id) == Some(self.id) {
                true => state.umount_tree(&self.root, false),
                false => Vec::new(),
            }
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
    /// Held while a write access is taken, and while a mount or filesystem is made read-only
    /// after checking it has none (`MNT_WRITE_HOLD`), so neither misses the other.
    write_hold: Mutex<()>,
}

impl Tree {
    pub(crate) fn new(seq: Arc<TreeSeq>) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::default(),
            seq,
            next_id: AtomicU64::new(1),
            write_hold: Mutex::new(()),
        })
    }

    /// `mnt_get_write_access`: `EROFS` if the mount or its filesystem is read-only.
    pub(crate) fn get_write_access(&self, mount: &Arc<Mount>) -> Result<WriteAccess, Errno> {
        let _hold = self.write_hold.lock_or_recover();
        if mount.is_read_only() {
            return Err(Errno::EROFS);
        }
        mount.writers.fetch_add(1, Ordering::SeqCst);
        mount.sb().writers.fetch_add(1, Ordering::SeqCst);
        Ok(WriteAccess(mount.clone()))
    }

    /// Makes `sb` read-only or writable (`reconfigure_super`); `EBUSY` if it would become
    /// read-only while a file is open for writing on it (`sb_prepare_remount_readonly`).
    pub(crate) fn set_sb_read_only(&self, sb: &Superblock, read_only: bool) -> Result<(), Errno> {
        let _hold = self.write_hold.lock_or_recover();
        if read_only && !sb.is_read_only() && sb.writers.load(Ordering::SeqCst) > 0 {
            return Err(Errno::EBUSY);
        }
        sb.set_read_only(read_only);
        Ok(())
    }

    /// Gives each of `mounts` the flags `change` makes of its own, all or none: `EBUSY` if one
    /// would become read-only while a file is open for writing through it
    /// (`mnt_hold_writers`, `mnt_make_readonly`).
    pub(crate) fn set_mount_flags(
        &self,
        mounts: &[Arc<Mount>],
        change: impl Fn(MountFlags) -> MountFlags,
    ) -> Result<(), Errno> {
        // `can_change_locked_flags`, for every mount first.
        if mounts
            .iter()
            .any(|mount| !mount.may_set_flags(&change(mount.flags())))
        {
            return Err(Errno::EPERM);
        }
        let _hold = self.write_hold.lock_or_recover();
        let busy = mounts.iter().any(|mount| {
            let old = mount.flags();
            change(old).read_only && !old.read_only && mount.writers.load(Ordering::SeqCst) > 0
        });
        if busy {
            return Err(Errno::EBUSY);
        }
        for mount in mounts {
            mount.set_flags(change(mount.flags()));
        }
        Ok(())
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
    /// The namespaces that are not anonymous, by id (`mnt_ns_tree`).
    pub(crate) namespaces: HashMap<u64, Weak<MntNs>>,
    /// The anonymous namespaces, by id, with the namespace each was copied from (`seq_origin`).
    pub(crate) anon_ns: HashMap<u64, u64>,
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

    /// `has_locked_children`: a locked mount is attached at or below `dentry` of `mount`.
    pub(crate) fn has_locked_children(&self, mount: &Mount, dentry: &Arc<Dentry>) -> bool {
        self.children_of(mount).iter().any(|child| {
            child.is_locked()
                && child
                    .parent()
                    .is_some_and(|at| dentry.is_ancestor_of(&at.dentry))
        })
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
