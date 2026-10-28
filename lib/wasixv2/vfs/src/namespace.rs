//! Changing mount trees: `mount(2)`'s operations, `umount2`, `pivot_root`, `mount_setattr`
//! and copying a namespace, after Linux's `fs/namespace.c`.
//!
//! The kernel resolves the paths in the caller's context and checks the caller may mount
//! (`may_mount`, `CAP_SYS_ADMIN`) before it calls these; each takes the caller's namespace,
//! which the mounts it changes must belong to (`check_mnt`).

use std::sync::{Arc, atomic::Ordering};

use wasixv2_abi::Errno;

use crate::{
    Vfs,
    dentry::Dentry,
    driver::Filesystem,
    file::File,
    mount::{MntNs, Mount, MountFlags, Path, Superblock, TreeState, is_path_reachable},
    pnode::{CopyFlags, Propagation},
    types::FsResult,
};

/// What `mount(2)` does to its target (`path_mount`'s dispatch).
pub enum MountOp {
    /// Mounts `fs`, made from `source` (`do_new_mount`), read-only as a filesystem if
    /// `sb_read_only` (`SB_RDONLY`) and it is not mounted already.
    New {
        fs: Arc<dyn Filesystem>,
        source: Vec<u8>,
        flags: MountFlags,
        sb_read_only: bool,
    },
    /// Mounts the tree at `source` again, with what is mounted below it if `recursive`
    /// (`do_loopback`, `MS_BIND` and `MS_REC`).
    Bind { source: Path, recursive: bool },
    /// Moves the mount whose root `source` is (`do_move_mount_old`, `MS_MOVE`), or attaches
    /// the detached tree whose root it is.
    Move { source: Path },
    /// Changes the filesystem's read-only state and the mount's flags (`do_remount`).
    Remount { read_only: bool, flags: MountFlags },
    /// Changes only the mount's flags (`do_reconfigure_mnt`, `MS_REMOUNT | MS_BIND`).
    Reconfigure { flags: MountFlags },
    /// Changes the propagation of the mount, and of all below it if `recursive`
    /// (`do_change_type`).
    Propagation { kind: Propagation, recursive: bool },
}

/// `umount2` flags.
#[derive(Clone, Copy, Debug, Default)]
pub struct UmountFlags {
    /// `MNT_FORCE`: no filesystem here has operations to abort, so it changes nothing.
    pub force: bool,
    /// `MNT_DETACH`: take the tree out now, whoever still uses it.
    pub detach: bool,
    /// `MNT_EXPIRE`: mark an unused mount, and unmount it if still marked next time.
    pub expire: bool,
}

/// `mount_setattr`'s change to a mount's flags and propagation (`struct mount_kattr`). The
/// flags set in `clear` are cleared, then those in `set` set (`recalc_flags`).
#[derive(Clone, Copy, Debug, Default)]
pub struct MountAttr {
    pub set: MountFlags,
    pub clear: MountFlags,
    pub propagation: Option<Propagation>,
    /// `AT_RECURSIVE`: every mount below too.
    pub recursive: bool,
}

/// A mount as `/proc/<pid>/mountinfo` shows it (`show_mountinfo`).
pub struct MountInfo {
    pub mount: Arc<Mount>,
    /// The mount it is attached on; its own id for the namespace root.
    pub parent: u64,
    /// `shared:N`: its peer group.
    pub shared: Option<u32>,
    /// `master:N`: the group it receives from.
    pub master: Option<u32>,
    /// `propagate_from:N`: the closest group it gets events from that has a member under the
    /// reader's root, when that is not its master.
    pub propagate_from: Option<u32>,
    pub unbindable: bool,
}

/// `cant_mount`: a removed name, or an object its filesystem keeps mounts off.
fn cant_mount(dentry: &Dentry) -> bool {
    dentry.is_unlinked() || dentry.node().unmountable()
}

fn check_mnt(state: &TreeState, ns: &MntNs, mount: &Mount) -> bool {
    state.ns_of(mount.id) == Some(ns.id())
}

/// `check_anonymous_mnt`: `mount` is in an anonymous namespace copied from the caller's, or
/// from none.
fn check_anonymous_mnt(state: &TreeState, ns: &MntNs, mount: &Mount) -> bool {
    let Some(anon) = state.ns_of(mount.id) else {
        return false;
    };
    state
        .anon_ns
        .get(&anon)
        .is_some_and(|&origin| origin == 0 || origin == ns.id())
}

/// `may_use_mount`: a mount of the caller's namespace, or of a detached tree it may use.
fn may_use_mount(state: &TreeState, ns: &MntNs, mount: &Mount) -> bool {
    check_mnt(state, ns, mount) || check_anonymous_mnt(state, ns, mount)
}

/// `anon_ns_root`: `mount` is the root of a detached tree.
fn anon_ns_root(state: &TreeState, mount: &Mount) -> bool {
    mount.parent().is_none()
        && state
            .ns_of(mount.id)
            .is_some_and(|ns| state.anon_ns.contains_key(&ns))
}

/// The mount `mount` is attached on, or itself for a root (`mnt_parent`).
fn parent_mount(mount: &Arc<Mount>) -> Arc<Mount> {
    mount.parent().map_or_else(|| mount.clone(), |at| at.mnt)
}

/// `mount_is_ancestor`: whether `mount` is `ancestor` or below it.
fn mount_is_ancestor(ancestor: &Arc<Mount>, mount: &Arc<Mount>) -> bool {
    let mut current = mount.clone();
    loop {
        if Arc::ptr_eq(&current, ancestor) {
            return true;
        }
        match current.parent() {
            Some(at) => current = at.mnt,
            None => return false,
        }
    }
}

impl Vfs {
    /// Mounts on `target` as `op` says (`path_mount`).
    pub fn do_mount(&self, ns: &MntNs, target: &Path, op: MountOp) -> FsResult<()> {
        match op {
            MountOp::New {
                fs,
                source,
                flags,
                sb_read_only,
            } => self.new_mount(ns, target, fs, source, flags, sb_read_only),
            MountOp::Bind { source, recursive } => self.loopback(ns, target, &source, recursive),
            MountOp::Move { source } => self.move_mount(ns, &source, target, false),
            MountOp::Remount { read_only, flags } => {
                let state = self.tree.lock();
                if !check_mnt(&state, ns, &target.mnt) || !target.is_mount_root() {
                    return Err(Errno::EINVAL);
                }
                // `reconfigure_super`, then `set_mount_attributes`.
                self.tree.set_sb_read_only(target.mnt.sb(), read_only)?;
                self.tree
                    .set_mount_flags(std::slice::from_ref(&target.mnt), |_| flags)
            }
            MountOp::Reconfigure { flags } => {
                let state = self.tree.lock();
                if !check_mnt(&state, ns, &target.mnt) || !target.is_mount_root() {
                    return Err(Errno::EINVAL);
                }
                // `do_reconfigure_mnt`: `mnt_make_readonly` while no file writes through it.
                self.tree
                    .set_mount_flags(std::slice::from_ref(&target.mnt), |_| flags)
            }
            MountOp::Propagation { kind, recursive } => {
                self.change_type(target, Some(kind), recursive, None)
            }
        }
    }

    /// `do_new_mount_fc` and `do_add_mount`.
    fn new_mount(
        &self,
        ns: &MntNs,
        target: &Path,
        fs: Arc<dyn Filesystem>,
        source: Vec<u8>,
        flags: MountFlags,
        sb_read_only: bool,
    ) -> FsResult<()> {
        let (root, new) = self.fs_root(fs);
        if new {
            root.sb().set_read_only(sb_read_only);
        }
        let mount = Arc::new(Mount::new(self.tree.next_id(), root, flags, source));
        let mut state = self.tree.lock();
        let at = self.lock_mount(&state, target)?;
        if !check_mnt(&state, ns, &at.mnt) {
            return Err(Errno::EINVAL);
        }
        state.add_link(&mount, 0);
        let result = self.graft_tree(&mut state, &mount, &at);
        if result.is_err() {
            state.umount_tree(&mount, false);
        }
        result
    }

    /// `do_lock_mount`: where a mount on `target` goes, on top of anything mounted there,
    /// unless that place takes no mounts.
    fn lock_mount(&self, state: &TreeState, target: &Path) -> FsResult<Path> {
        let at = state.where_to_mount(target);
        if cant_mount(&at.dentry) {
            return Err(Errno::ENOENT);
        }
        Ok(at)
    }

    /// `graft_tree`: attaches the new tree at `mount` on `at`.
    fn graft_tree(&self, state: &mut TreeState, mount: &Arc<Mount>, at: &Path) -> FsResult<()> {
        if at.dentry.is_dir() != mount.root.is_dir() {
            return Err(Errno::ENOTDIR);
        }
        self.attach_recursive(state, mount, at, false)
    }

    /// `attach_recursive_mnt`: attaches the tree at `source` on `at`, with copies for every
    /// mount that receives from `at`'s mount if that is shared. A tree that is `moving` is
    /// taken off where it was.
    fn attach_recursive(
        &self,
        state: &mut TreeState,
        source: &Arc<Mount>,
        at: &Path,
        moving: bool,
    ) -> FsResult<()> {
        let dest = &at.mnt;
        let ns = state.ns_of(dest.id).unwrap_or(0);
        let tree = state.subtree(source);
        let shared = state.is_shared(dest);
        let mut copies = Vec::new();
        if shared {
            let invented = state.invent_group_ids(&tree);
            match state.propagate_mnt(&self.tree.next_id, dest, &at.dentry, source) {
                Ok(made) => copies = made,
                Err(errno) => {
                    state.cleanup_group_ids(&invented);
                    return Err(errno);
                }
            }
        }
        self.tree.seq.bump();
        if moving {
            state.detach(source);
        }
        // The original goes first, then each copy, in the same way (`commit_tree`).
        let originals = std::iter::once((at.clone(), source.clone()));
        for (place, child) in originals.chain(copies) {
            let covering = state.mounted_on(&place);
            if let Some(covering) = &covering {
                state.detach(covering);
            }
            state.attach(&child, &place);
            let attached = state.subtree(&child);
            let place_ns = match Arc::ptr_eq(&child, source) {
                true => ns,
                false => state.ns_of(place.mnt.id).unwrap_or(0),
            };
            state.set_ns(&attached, place_ns);
            // What was there goes on top of the new tree's topmost mount: a receiver's mount
            // slides over the copy, the mount a move goes beneath over the moved tree
            // (`mnt_change_mountpoint`).
            if let Some(covering) = covering {
                let top = state.topmost(&child);
                state.attach(&covering, &top.root_path());
            }
        }
        Ok(())
    }

    /// `do_loopback`.
    fn loopback(&self, ns: &MntNs, target: &Path, source: &Path, recursive: bool) -> FsResult<()> {
        let mut state = self.tree.lock();
        let at = self.lock_mount(&state, target)?;
        if !check_mnt(&state, ns, &at.mnt) {
            return Err(Errno::EINVAL);
        }
        let mount = self.copy_for_loopback(&mut state, ns, source, recursive)?;
        let result = self.graft_tree(&mut state, &mount, &at);
        if result.is_err() {
            state.umount_tree(&mount, false);
        }
        result
    }

    /// `__do_loopback`: a copy of the tree at `source` (with what is mounted below it if
    /// `recursive`), attached nowhere. The source must be bindable, a mount of the caller's
    /// namespace or of a detached tree it may use (`may_copy_tree`), and not an object without
    /// a name.
    fn copy_for_loopback(
        &self,
        state: &mut TreeState,
        ns: &MntNs,
        source: &Path,
        recursive: bool,
    ) -> FsResult<Arc<Mount>> {
        let old = &source.mnt;
        if state.is_unbindable(old) || !may_use_mount(state, ns, old) || source.dentry.is_pseudo() {
            return Err(Errno::EINVAL);
        }
        // What is below a mount no longer in any tree is not known.
        if recursive && state.ns_of(old.id).is_none() {
            return Err(Errno::EINVAL);
        }
        let flags = CopyFlags::default();
        match recursive {
            true => Ok(state
                .copy_tree(&self.tree.next_id, old, &source.dentry, flags)?
                .0),
            false => Ok(state.clone_mnt(&self.tree.next_id, old, source.dentry.clone(), flags)),
        }
    }

    /// `do_lock_mount` with `beneath`: the place under the topmost mount stacked on the mount
    /// whose root `target` is, with that mount (`where_to_mount`).
    fn lock_mount_beneath(&self, state: &TreeState, target: &Path) -> FsResult<(Path, Arc<Mount>)> {
        if !target.is_mount_root() {
            return Err(Errno::EINVAL);
        }
        let over = state.topmost(&target.mnt);
        // Nothing can go beneath a namespace's root.
        let at = over.parent().ok_or(Errno::EINVAL)?;
        if cant_mount(&at.dentry) {
            return Err(Errno::ENOENT);
        }
        Ok((at, over))
    }

    /// `do_move_mount`: moves the mount whose root `source` is, with its tree, onto `target`,
    /// or beneath the topmost mount on it (`MOVE_MOUNT_BENEATH`). A mount of the caller's
    /// namespace moves within it; the root of a detached tree (`open_tree`, `fsmount`) is
    /// attached, its whole tree leaving its anonymous namespace.
    pub fn move_mount(
        &self,
        ns: &MntNs,
        source: &Path,
        target: &Path,
        beneath: bool,
    ) -> FsResult<()> {
        if !source.is_mount_root() {
            return Err(Errno::EINVAL);
        }
        if target.dentry.is_dir() != source.dentry.is_dir() {
            return Err(Errno::EINVAL);
        }
        let mut state = self.tree.lock();
        let (at, over) = match beneath {
            true => {
                let (at, over) = self.lock_mount_beneath(&state, target)?;
                (at, Some(over))
            }
            false => (self.lock_mount(&state, target)?, None),
        };
        let old = &source.mnt;
        if state.ns_of(at.mnt.id).is_none() {
            return Err(Errno::ENOENT);
        }
        if check_mnt(&state, ns, old) {
            // A mount of the caller's namespace: it must have a parent, which must not
            // propagate, and stay in the namespace.
            let Some(from) = old.parent() else {
                return Err(Errno::EINVAL);
            };
            if state.is_shared(&from.mnt) || !check_mnt(&state, ns, &at.mnt) {
                return Err(Errno::EINVAL);
            }
        } else {
            // Otherwise the root of a detached tree, going into another namespace the caller
            // may use.
            if !anon_ns_root(&state, old) || state.ns_of(old.id) == state.ns_of(at.mnt.id) {
                return Err(Errno::EINVAL);
            }
            if !may_use_mount(&state, ns, &at.mnt) {
                return Err(Errno::EINVAL);
            }
        }
        if let Some(over) = &over {
            can_move_mount_beneath(&state, ns, old, over, &at)?;
        }
        let tree = state.subtree(old);
        if state.is_shared(&at.mnt) && tree.iter().any(|m| state.is_unbindable(m)) {
            return Err(Errno::EINVAL);
        }
        if mount_is_ancestor(old, &at.mnt) {
            return Err(Errno::ELOOP);
        }
        let moving = old.parent().is_some();
        self.attach_recursive(&mut state, old, &at, moving)
    }

    /// `do_set_group` (`MOVE_MOUNT_SET_GROUP`): the private mount whose root `to` is joins the
    /// peer group of the mount whose root `from` is, and receives from its master.
    pub fn set_group(&self, from: &Path, to: &Path) -> FsResult<()> {
        let mut state = self.tree.lock();
        // `may_change_propagation`.
        if state.ns_of(from.mnt.id).is_none() || state.ns_of(to.mnt.id).is_none() {
            return Err(Errno::EINVAL);
        }
        if !from.is_mount_root() || !to.is_mount_root() {
            return Err(Errno::EINVAL);
        }
        if !Arc::ptr_eq(from.mnt.sb(), to.mnt.sb()) {
            return Err(Errno::EINVAL);
        }
        // From's root must be wider than to's.
        if !from.mnt.root.is_ancestor_of(&to.mnt.root) {
            return Err(Errno::EINVAL);
        }
        let (to_group, to_master, to_unbindable) = state.propagation_of(&to.mnt);
        if to_group != 0 || to_master != 0 || to_unbindable {
            return Err(Errno::EINVAL);
        }
        let (group, master, _) = state.propagation_of(&from.mnt);
        if group == 0 && master == 0 {
            return Err(Errno::EINVAL);
        }
        state.set_propagation(&to.mnt, group, master);
        self.tree.seq.bump();
        Ok(())
    }

    /// `open_detached_copy` (`open_tree(OPEN_TREE_CLONE)`): a copy of the tree at `path`, with
    /// what is mounted below it if `recursive`, in an anonymous namespace of its own, as an
    /// `O_PATH` file at its root. Closing the file unmounts the copy unless `move_mount`
    /// attached it.
    pub fn open_tree(self: &Arc<Self>, ns: &MntNs, path: &Path, recursive: bool) -> FsResult<File> {
        let detached = {
            let mut state = self.tree.lock();
            // `seq_origin`: the namespace the tree comes from, or the one its own came from.
            let origin = match state.ns_of(path.mnt.id) {
                Some(from) => state.anon_ns.get(&from).copied().unwrap_or(from),
                None => 0,
            };
            let mount = self.copy_for_loopback(&mut state, ns, path, recursive)?;
            let anon = MntNs::new(&self.tree, &mut state, mount.clone(), Some(origin));
            let tree = state.subtree(&mount);
            state.set_ns(&tree, anon.id());
            anon
        };
        Ok(File::detached(self.clone(), detached))
    }

    /// `fsmount`: a new mount of `fs` made from `source`, with `flags`, as the only mount of an
    /// anonymous namespace, as an `O_PATH` file at its root (see [`Vfs::open_tree`]). The
    /// filesystem is read-only if `sb_read_only` and it is not mounted already.
    pub fn create_mount(
        self: &Arc<Self>,
        fs: Arc<dyn Filesystem>,
        source: Vec<u8>,
        flags: MountFlags,
        sb_read_only: bool,
    ) -> File {
        let (root, new) = self.fs_root(fs);
        if new {
            root.sb().set_read_only(sb_read_only);
        }
        let mount = Arc::new(Mount::new(self.tree.next_id(), root, flags, source));
        let detached = {
            let mut state = self.tree.lock();
            let anon = MntNs::new(&self.tree, &mut state, mount.clone(), Some(0));
            state.add_link(&mount, anon.id());
            anon
        };
        File::detached(self.clone(), detached)
    }

    /// `do_change_type`, and the propagation part of `mount_setattr`: `kind` for the mount
    /// whose root `target` is and, if `recursive`, every mount below it. `flags`, if given,
    /// replaces the flags of each the same way.
    fn change_type(
        &self,
        target: &Path,
        kind: Option<Propagation>,
        recursive: bool,
        attr: Option<&MountAttr>,
    ) -> FsResult<()> {
        if !target.is_mount_root() {
            return Err(Errno::EINVAL);
        }
        let mut state = self.tree.lock();
        // `may_change_propagation`: the mount must be in some namespace.
        if state.ns_of(target.mnt.id).is_none() {
            return Err(Errno::EINVAL);
        }
        let mounts = match recursive {
            true => state.subtree(&target.mnt),
            false => vec![target.mnt.clone()],
        };
        // `mount_setattr_prepare`: the flags change on every mount or none.
        if let Some(attr) = attr {
            self.tree
                .set_mount_flags(&mounts, |old| recalc_flags(old, attr))?;
        }
        if kind == Some(Propagation::Shared) {
            state.invent_group_ids(&mounts);
        }
        if let Some(kind) = kind {
            for mount in &mounts {
                state.change_propagation(mount.id, kind);
            }
        }
        self.tree.seq.bump();
        Ok(())
    }

    /// `do_mount_setattr`: changes the flags and propagation of the mount whose root `path` is
    /// (and of every mount below with `attr.recursive`).
    pub fn mount_setattr(&self, ns: &MntNs, path: &Path, attr: &MountAttr) -> FsResult<()> {
        if !path.is_mount_root() {
            return Err(Errno::EINVAL);
        }
        if !check_mnt(&self.tree.lock(), ns, &path.mnt) {
            return Err(Errno::EINVAL);
        }
        self.change_type(path, attr.propagation, attr.recursive, Some(attr))
    }

    /// `path_umount`: unmounts the mount whose root `path` is. The caller holds one reference to
    /// it, through `path`; any other besides the tree's makes it busy unless detaching. A
    /// caller unmounting its own root without `MNT_DETACH` makes the filesystem read-only
    /// instead (`do_umount_root`).
    pub fn do_umount(
        &self,
        ns: &MntNs,
        caller_root: &Path,
        path: &Path,
        flags: UmountFlags,
    ) -> FsResult<()> {
        let mount = &path.mnt;
        // `can_umount`.
        if !path.is_mount_root() || !check_mnt(&self.tree.lock(), ns, mount) {
            return Err(Errno::EINVAL);
        }
        let is_root = Arc::ptr_eq(mount, &caller_root.mnt);
        if flags.expire {
            if is_root || flags.force || flags.detach {
                return Err(Errno::EINVAL);
            }
            let state = self.tree.lock();
            if state.has_children(mount) || Arc::strong_count(mount) != 2 {
                return Err(Errno::EBUSY);
            }
            if !mount.expiry_mark.swap(true, Ordering::Relaxed) {
                return Err(Errno::EAGAIN);
            }
        }
        if is_root && !flags.detach {
            // `do_umount_root`.
            return self.tree.set_sb_read_only(mount.sb(), true);
        }
        let _gone = {
            let mut state = self.tree.lock();
            if state.ns_of(mount.id) != Some(ns.id()) || mount.parent().is_none() {
                return Err(Errno::EINVAL);
            }
            // The tree's reference and the caller's.
            if !flags.detach && state.propagate_mount_busy(mount, 2) {
                return Err(Errno::EBUSY);
            }
            self.tree.seq.bump();
            state.umount_tree(mount, true)
        };
        Ok(())
    }

    /// `path_pivot_root`: in the caller's namespace, the mount at `new_root` takes the place of
    /// the caller's root mount (`root`), which moves to `put_old`. The kernel then moves every
    /// process whose root or working directory was `root` to `new_root` (`chroot_fs_refs`).
    pub fn pivot_root(
        &self,
        ns: &MntNs,
        root: &Path,
        new_root: &Path,
        put_old: &Path,
    ) -> FsResult<()> {
        let mut state = self.tree.lock();
        let old_at = self.lock_mount(&state, put_old)?;
        let old_mnt = &old_at.mnt;
        let new_mnt = &new_root.mnt;
        let root_mnt = &root.mnt;
        if !check_mnt(&state, ns, root_mnt) || !check_mnt(&state, ns, new_mnt) {
            return Err(Errno::EINVAL);
        }
        if state.is_shared(old_mnt)
            || state.is_shared(&parent_mount(new_mnt))
            || state.is_shared(&parent_mount(root_mnt))
        {
            return Err(Errno::EINVAL);
        }
        if new_root.dentry.is_unlinked() {
            return Err(Errno::ENOENT);
        }
        if Arc::ptr_eq(new_mnt, root_mnt) || Arc::ptr_eq(old_mnt, root_mnt) {
            return Err(Errno::EBUSY);
        }
        if !root.is_mount_root() || root_mnt.parent().is_none() {
            return Err(Errno::EINVAL);
        }
        if !new_root.is_mount_root() || new_mnt.parent().is_none() {
            return Err(Errno::EINVAL);
        }
        if !is_path_reachable(old_mnt, &old_at.dentry, new_root) {
            return Err(Errno::EINVAL);
        }
        if !is_path_reachable(new_mnt, &new_root.dentry, root) {
            return Err(Errno::EINVAL);
        }
        self.tree.seq.bump();
        let root_mp = state.detach(root_mnt).expect("checked above");
        state.detach(new_mnt);
        state.attach(new_mnt, &root_mp);
        state.attach(root_mnt, &old_at);
        Ok(())
    }

    /// `copy_mnt_ns`: see [`MntNs::copy`].
    pub fn copy_mnt_ns(&self, from: &MntNs, paths: &mut [&mut Path]) -> Arc<MntNs> {
        from.copy(paths)
    }

    /// The mounts of `ns`, oldest first, with their propagation as seen from `root`.
    pub fn mount_info(&self, ns: &MntNs, root: &Path) -> Vec<MountInfo> {
        let state = self.tree.lock();
        state
            .mounts_in(ns.id())
            .into_iter()
            .map(|mount| {
                let (group, master, unbindable) = state.propagation_of(&mount);
                let dominating = match master {
                    0 => 0,
                    _ => state.dominating_group(&mount, root),
                };
                MountInfo {
                    parent: mount.parent().map_or(mount.id, |at| at.mnt.id),
                    shared: (group != 0).then_some(group),
                    master: (master != 0).then_some(master),
                    propagate_from: (dominating != 0 && dominating != master).then_some(dominating),
                    unbindable,
                    mount,
                }
            })
            .collect()
    }

    /// Every filesystem mounted in some namespace, once each (`iterate_supers`).
    pub fn superblocks(&self) -> Vec<Arc<Superblock>> {
        let state = self.tree.lock();
        let mut found: Vec<Arc<Superblock>> = Vec::new();
        for link in state.links.values() {
            if let Some(mount) = link.mount.upgrade()
                && link.ns != 0
                && !found.iter().any(|sb| Arc::ptr_eq(sb, mount.sb()))
            {
                found.push(mount.sb().clone());
            }
        }
        found.sort_by_key(|sb| sb.id);
        found
    }

    /// Whether something in `path`'s namespace is mounted on its dentry: removing or renaming
    /// it is then `EBUSY` (`is_local_mountpoint`).
    pub(crate) fn is_local_mountpoint(&self, path: &Path) -> bool {
        if !path.dentry.is_mountpoint() {
            return false;
        }
        let state = self.tree.lock();
        state
            .ns_of(path.mnt.id)
            .is_some_and(|ns| state.is_local_mountpoint(ns, &path.dentry))
    }

    /// `detach_mounts`: lazily unmounts what other namespaces have mounted on `dentry`, whose
    /// name is gone.
    pub(crate) fn detach_mounts(&self, dentry: &Arc<Dentry>) {
        if !dentry.is_mountpoint() {
            return;
        }
        let _gone: Vec<_> = {
            let mut state = self.tree.lock();
            self.tree.seq.bump();
            state
                .mounted_on_dentry(dentry)
                .iter()
                .flat_map(|mount| state.umount_tree(mount, true))
                .collect()
        };
    }
}

/// `can_move_mount_beneath`: `old` can go under `over`, the topmost mount on `at`, without
/// propagation putting a copy of it on top of `over` or of itself.
fn can_move_mount_beneath(
    state: &TreeState,
    ns: &MntNs,
    old: &Arc<Mount>,
    over: &Arc<Mount>,
    at: &Path,
) -> FsResult<()> {
    // No shadow mounts: nothing may be on top of the moved mount.
    if state.mounted_on(&old.root_path()).is_some() {
        return Err(Errno::EINVAL);
    }
    if mount_is_ancestor(over, old) {
        return Err(Errno::EINVAL);
    }
    if state.propagation_would_overmount(&at.mnt, over, &at.dentry) {
        return Err(Errno::EINVAL);
    }
    if check_mnt(state, ns, old) && state.propagation_would_overmount(&at.mnt, old, &at.dentry) {
        return Err(Errno::EINVAL);
    }
    Ok(())
}

/// `recalc_flags`.
fn recalc_flags(old: MountFlags, attr: &MountAttr) -> MountFlags {
    let pick = |old: bool, clear: bool, set: bool| (old && !clear) || set;
    let (set, clear) = (&attr.set, &attr.clear);
    MountFlags {
        read_only: pick(old.read_only, clear.read_only, set.read_only),
        nosuid: pick(old.nosuid, clear.nosuid, set.nosuid),
        nodev: pick(old.nodev, clear.nodev, set.nodev),
        noexec: pick(old.noexec, clear.noexec, set.noexec),
        noatime: pick(old.noatime, clear.noatime, set.noatime),
        nodiratime: pick(old.nodiratime, clear.nodiratime, set.nodiratime),
        strictatime: pick(old.strictatime, clear.strictatime, set.strictatime),
        nosymfollow: pick(old.nosymfollow, clear.nosymfollow, set.nosymfollow),
    }
}

impl MntNs {
    /// `copy_mnt_ns`: a new namespace holding a copy of every mount of this one, shared mounts
    /// joining their originals' peer groups and slaves their masters. Each of `paths` (the new
    /// root and working directory) moves to the copy of its mount.
    pub fn copy(&self, paths: &mut [&mut Path]) -> Arc<MntNs> {
        let mut state = self.tree.lock();
        let flags = CopyFlags {
            copy_unbindable: true,
            ..CopyFlags::default()
        };
        let source = self.root().mnt;
        let (root, copies) = state
            .copy_tree(&self.tree.next_id, &source, &source.root, flags)
            .expect("copying unbindable mounts too, a whole tree copies");
        let ns = MntNs::new(&self.tree, &mut state, root, None);
        let copied: Vec<_> = copies.iter().map(|(_, copy)| copy.clone()).collect();
        state.set_ns(&copied, ns.id());
        for path in paths {
            if let Some((_, copy)) = copies
                .iter()
                .find(|(original, _)| Arc::ptr_eq(original, &path.mnt))
            {
                path.mnt = copy.clone();
            }
        }
        ns
    }

    /// The root a process entering the namespace gets: the namespace's root, below whatever
    /// is mounted on it (`mntns_install`'s `follow_down_one` loop).
    pub fn entry_root(&self) -> Path {
        let mut root = self.root();
        self.tree.follow_mounts(&mut root);
        root
    }
}
